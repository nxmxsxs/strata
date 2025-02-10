// Copyright 2023 the Strata authors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
	ffi::OsString,
	sync::Arc,
	time::Instant,
};

use anyhow::Context as _;
use input::{
	Modifier,
	Mods,
};
use piccolo::{
	self as lua,
};
use piccolo_util::freeze::{
	Freeze,
	FreezeGuard,
	Frozen,
};
use smithay::{
	backend::input::{
		InputBackend,
		InputEvent,
	},
	desktop::{
		PopupManager,
		Space,
		Window,
		layer_map_for_output,
	},
	input::{
		Seat,
		SeatState,
		keyboard::XkbConfig,
	},
	reexports::{
		calloop::{
			EventLoop,
			Interest,
			LoopHandle,
			LoopSignal,
			Mode,
			PostAction,
			generic::Generic,
		},
		wayland_server::{
			Display,
			DisplayHandle,
			backend::{
				ClientData,
				ClientId,
				DisconnectReason,
			},
		},
	},
	utils::{
		Logical,
		Point,
	},
	wayland::{
		compositor::{
			CompositorClientState,
			CompositorState,
		},
		output::OutputManagerState,
		selection::{
			data_device::DataDeviceState,
			primary_selection::PrimarySelectionState,
		},
		shell::{
			wlr_layer::{
				Layer,
				WlrLayerShellState,
			},
			xdg::{
				XdgShellState,
				decoration::XdgDecorationState,
			},
		},
		shm::ShmState,
		socket::ListeningSocketSource,
	},
};

use crate::{
	api::{
		self,
	},
	backends::Backend,
	config::StrataConfig,
	workspaces::{
		FocusTarget,
		Workspaces,
	},
};

pub mod input;
mod process;

pub type FrozenCompositor = Frozen<Freeze![&'freeze mut Compositor]>;

pub struct Runtime {
	lua: lua::Lua,
	ex: lua::StashedExecutor,
	fcomp: FrozenCompositor,
}

impl Runtime {
	pub fn enter<R>(&mut self, f: impl FnOnce(lua::Context, &lua::StashedExecutor) -> R) -> R {
		self.lua.enter(|ctx| f(ctx, &self.ex))
	}

	pub fn execute<R>(&mut self, comp: &mut Compositor) -> anyhow::Result<R>
	where
		R: for<'gc> lua::FromMultiValue<'gc>,
	{
		FreezeGuard::new(&self.fcomp, comp).scope(|| self.lua.execute(&self.ex).context("lua closure error"))
	}

	pub fn execute_closure<R, const N: usize>(
		&mut self,
		comp: &mut Compositor,
		f: impl for<'gc> FnOnce(lua::Context<'gc>, &mut Compositor) -> (lua::Function<'gc>, [lua::Value<'gc>; N]),
	) -> anyhow::Result<R>
	where
		R: for<'gc> lua::FromMultiValue<'gc>,
	{
		self.enter(|ctx, ex| {
			let (f, args) = f(ctx, comp);
			ctx.fetch(ex).restart(ctx, f, lua::Variadic(args));
		});
		self.execute::<R>(comp).context("lua closure error")
	}

	pub fn try_execute_closure<R, const N: usize>(
		&mut self,
		comp: &mut Compositor,
		f: impl for<'gc> FnOnce(lua::Context<'gc>, &mut Compositor) -> Option<(lua::Function<'gc>, [lua::Value<'gc>; N])>,
	) -> Option<anyhow::Result<R>>
	where
		R: for<'gc> lua::FromMultiValue<'gc>,
	{
		self.enter(|ctx, ex| f(ctx, comp).map(|(f, args)| ctx.fetch(ex).restart(ctx, f, lua::Variadic(args))))
			.map(|_| self.execute::<R>(comp).context("lua closure error"))
	}
}

pub struct Strata {
	pub comp: Compositor,
	pub rt: Runtime,
}

impl Strata {
	pub fn new(mut comp: Compositor) -> anyhow::Result<Self> {
		unsafe { std::env::set_var("WAYLAND_DISPLAY", &comp.socket_name) };

		process::init_sigchld_handler()?;

		let mut lua = lua::Lua::full();
		let ex = lua.enter(|ctx| ctx.stash(lua::Executor::new(ctx)));
		let fcomp = FrozenCompositor::new();
		let mut rt = Runtime {
			lua,
			ex,
			fcomp: fcomp.clone(),
		};

		rt.enter(|ctx, ex| {
			ctx.globals().set(ctx, "strata", api::create_global(ctx, fcomp)?)?;

			let main = lua::Closure::load(ctx, None, include_str!("../init.lua").as_bytes())?;

			ctx.fetch(ex).restart(ctx, main.into(), ());

			anyhow::Ok(())
		})?;
		if let Err(e) = rt.execute::<()>(&mut comp) {
			println!("{:?}", e);
		}

		Ok(Strata {
			comp,
			rt,
		})
	}

	pub fn enter<R>(&mut self, f: impl FnOnce(lua::Context, &lua::StashedExecutor, &mut Compositor) -> R) -> R {
		self.rt.enter(|ctx, ex| f(ctx, ex, &mut self.comp))
	}

	pub fn execute<R: for<'gc> lua::FromMultiValue<'gc>>(&mut self) -> anyhow::Result<R> {
		self.rt.execute(&mut self.comp)
	}

	pub fn execute_closure<R, const N: usize>(
		&mut self,
		f: impl for<'gc> FnOnce(lua::Context<'gc>, &mut Compositor) -> (lua::Function<'gc>, [lua::Value<'gc>; N]),
	) -> anyhow::Result<R>
	where
		R: for<'gc> lua::FromMultiValue<'gc>,
	{
		self.rt.execute_closure(&mut self.comp, f)
	}

	pub fn try_execute_closure<R, const N: usize>(
		&mut self,
		f: impl for<'gc> FnOnce(lua::Context<'gc>, &mut Compositor) -> Option<(lua::Function<'gc>, [lua::Value<'gc>; N])>,
	) -> Option<anyhow::Result<R>>
	where
		R: for<'gc> lua::FromMultiValue<'gc>,
	{
		self.rt.try_execute_closure(&mut self.comp, f)
	}

	pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) -> anyhow::Result<()> {
		match event {
			InputEvent::Keyboard {
				event, ..
			} => self.on_keyboard::<I>(event)?,
			InputEvent::PointerMotion {
				event, ..
			} => self.comp.pointer_motion::<I>(event)?,
			InputEvent::PointerMotionAbsolute {
				event, ..
			} => self.comp.pointer_motion_absolute::<I>(event)?,
			InputEvent::PointerButton {
				event, ..
			} => self.comp.pointer_button::<I>(event)?,
			InputEvent::PointerAxis {
				event, ..
			} => self.comp.pointer_axis::<I>(event)?,
			InputEvent::DeviceAdded {
				device: _,
			} => {
				// todo
				println!("device added");
			}
			InputEvent::DeviceRemoved {
				device: _,
			} => todo!(),
			InputEvent::GestureSwipeBegin {
				event: _,
			} => todo!(),
			InputEvent::GestureSwipeUpdate {
				event: _,
			} => todo!(),
			InputEvent::GestureSwipeEnd {
				event: _,
			} => todo!(),
			InputEvent::GesturePinchBegin {
				event: _,
			} => todo!(),
			InputEvent::GesturePinchUpdate {
				event: _,
			} => todo!(),
			InputEvent::GesturePinchEnd {
				event: _,
			} => todo!(),
			InputEvent::GestureHoldBegin {
				event: _,
			} => todo!(),
			InputEvent::GestureHoldEnd {
				event: _,
			} => todo!(),
			InputEvent::TouchDown {
				event: _,
			} => todo!(),
			InputEvent::TouchMotion {
				event: _,
			} => todo!(),
			InputEvent::TouchUp {
				event: _,
			} => todo!(),
			InputEvent::TouchCancel {
				event: _,
			} => todo!(),
			InputEvent::TouchFrame {
				event: _,
			} => todo!(),
			InputEvent::TabletToolAxis {
				event: _,
			} => todo!(),
			InputEvent::TabletToolProximity {
				event: _,
			} => todo!(),
			InputEvent::TabletToolTip {
				event: _,
			} => todo!(),
			InputEvent::TabletToolButton {
				event: _,
			} => todo!(),
			InputEvent::Special(_) => todo!(),
			// _ => anyhow::bail!("unhandled winit event: {:#?}", &event),
		};

		Ok(())
	}
}

pub struct Compositor {
	pub backend: Backend,

	pub display_handle: DisplayHandle,
	pub loop_handle: LoopHandle<'static, Strata>,
	pub loop_signal: LoopSignal,

	pub clock: Instant,

	pub compositor_state: CompositorState,
	pub xdg_shell_state: XdgShellState,
	pub xdg_decoration_state: XdgDecorationState,
	pub shm_state: ShmState,
	pub output_manager_state: OutputManagerState,
	pub data_device_state: DataDeviceState,
	pub primary_selection_state: PrimarySelectionState,
	pub seat_state: SeatState<Compositor>,
	pub layer_shell_state: WlrLayerShellState,
	pub popup_manager: PopupManager,
	pub space: Space<Window>,
	pub seat: Seat<Compositor>,
	pub socket_name: OsString,
	pub workspaces: Workspaces,
	pub mods: input::Mods,

	pub config: StrataConfig,
	pub process_state: process::ProcessState,
}

impl Compositor {
	pub fn new(event_loop: &EventLoop<'static, Strata>) -> anyhow::Result<Self> {
		let display = Display::new()?;
		let loop_handle = event_loop.handle();
		let display_handle = display.handle();

		let listening_socket = ListeningSocketSource::new_auto().unwrap();
		let socket_name = listening_socket.socket_name().to_os_string();
		loop_handle
			.insert_source(listening_socket, move |client_stream, _, strata| {
				// You may also associate some data with the client when inserting the client.
				strata
					.comp
					.display_handle
					.insert_client(client_stream, Arc::new(ClientState::default()))
					.unwrap();
			})
			.expect("Failed to init the wayland event source.");

		loop_handle
			.insert_source(
				Generic::new(display, Interest::READ, Mode::Level),
				|_, display, strata| {
					// Safety: display isn't moved or dropped
					let display = unsafe { display.get_mut() };

					display.dispatch_clients(&mut strata.comp)?;
					Ok(PostAction::Continue)
				},
			)
			.unwrap();

		let config = StrataConfig::default();

		let repeat_info = &config.input_config.repeat_info;
		let xkbconfig = &config
			.input_config
			.xkbconfig
			.as_ref()
			.expect("StrataXkbConfig should have a default set");

		let mut seat_state = SeatState::new();
		let mut seat = seat_state.new_wl_seat(&display_handle, "strata-seat-0");
		let keyboard = seat
			.add_keyboard(
				XkbConfig {
					layout: &xkbconfig.layout,
					options: xkbconfig.options.clone(),
					rules: &xkbconfig.rules,
					model: &xkbconfig.model,
					variant: &xkbconfig.variant,
				},
				repeat_info.delay,
				repeat_info.rate,
			)
			.expect("Couldn't parse XKB config");
		seat.add_pointer();

		let config_workspace: u8 = 5;
		let workspaces = Workspaces::new(config_workspace);
		let mods_state = keyboard.modifier_state();

		let compositor_state = CompositorState::new::<Compositor>(&display_handle);
		let xdg_shell_state = XdgShellState::new::<Compositor>(&display_handle);
		let xdg_decoration_state = XdgDecorationState::new::<Compositor>(&display_handle);
		let shm_state = ShmState::new::<Compositor>(&display_handle, vec![]);
		let output_manager_state = OutputManagerState::new_with_xdg_output::<Compositor>(&display_handle);
		let data_device_state = DataDeviceState::new::<Compositor>(&display_handle);
		let primary_selection_state = PrimarySelectionState::new::<Compositor>(&display_handle);
		let layer_shell_state = WlrLayerShellState::new::<Compositor>(&display_handle);

		let process_state = process::ProcessState::new(&loop_handle)?;

		let comp = Compositor {
			backend: Backend::Unset,
			display_handle,
			loop_handle,
			loop_signal: event_loop.get_signal(),

			clock: Instant::now(),

			compositor_state,
			xdg_shell_state,
			xdg_decoration_state,
			shm_state,
			output_manager_state,
			data_device_state,
			primary_selection_state,
			seat_state,
			layer_shell_state,
			popup_manager: PopupManager::default(),
			space: Space::<Window>::default(),
			seat,
			socket_name,
			workspaces,
			mods: Mods {
				flags: Modifier::empty(),
				state: mods_state,
			},

			config,
			process_state,
		};

		Ok(comp)
	}

	pub fn surface_under(&self) -> Option<(FocusTarget, Point<i32, Logical>)> {
		let pos = self.seat.get_pointer().unwrap().current_location();
		let output = self.workspaces.current().outputs().find(|o| {
			let geometry = self.workspaces.current().output_geometry(o).unwrap();
			geometry.contains(pos.to_i32_round())
		})?;
		let output_geo = self.workspaces.current().output_geometry(output).unwrap();
		let layers = layer_map_for_output(output);

		let mut under = None;
		if let Some(layer) = layers
			.layer_under(Layer::Overlay, pos)
			.or_else(|| layers.layer_under(Layer::Top, pos))
		{
			let layer_loc = layers.layer_geometry(layer).unwrap().loc;
			under = Some((layer.clone().into(), output_geo.loc + layer_loc))
		} else if let Some((window, location)) = self.workspaces.current().window_under(pos) {
			under = Some((window.clone().into(), location));
		} else if let Some(layer) = layers
			.layer_under(Layer::Bottom, pos)
			.or_else(|| layers.layer_under(Layer::Background, pos))
		{
			let layer_loc = layers.layer_geometry(layer).unwrap().loc;
			under = Some((layer.clone().into(), output_geo.loc + layer_loc));
		};
		under
	}

	pub fn close_window(&mut self) {
		let ptr = self.seat.get_pointer().unwrap();
		if let Some((window, _)) = self.workspaces.current().window_under(ptr.current_location()) {
			window.toplevel().send_close()
		}
	}

	pub fn switch_to_workspace(&mut self, id: u8) {
		self.workspaces.activate(id);
		self.set_input_focus_auto();
	}

	pub fn move_window_to_workspace(&mut self, id: u8) {
		let pos = self.seat.get_pointer().unwrap().current_location();
		let window = self.workspaces.current().window_under(pos).map(|d| d.0.clone());

		if let Some(window) = window {
			self.workspaces.move_window_to_workspace(&window, id);
		}
	}

	pub fn follow_window_move(&mut self, id: u8) {
		self.move_window_to_workspace(id);
		self.switch_to_workspace(id);
	}

	pub fn quit(&self) {
		self.loop_signal.stop();
	}
}

pub trait WithState {
	type State;

	fn with_state<F, T>(&self, f: F)
	where
		F: FnOnce(&Self::State) -> T;

	fn with_state_mut<F, T>(&self, f: F)
	where
		F: FnOnce(&mut Self::State) -> T;
}

#[derive(Default)]
pub struct ClientState {
	pub compositor_state: CompositorClientState,
}
impl ClientData for ClientState {
	fn initialized(&self, _client_id: ClientId) {}

	fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
