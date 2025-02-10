use std::time::Instant;

use anyhow::Context as _;
use bitflags::bitflags;
use piccolo::{
	self as lua,
};
use smithay::{
	backend::input::{
		Event,
		InputBackend,
		KeyState,
		KeyboardKeyEvent,
	},
	input::keyboard::{
		FilterResult,
		KeyboardHandle,
		Keysym,
		ModifiersState,
		XkbConfig,
	},
	utils::SERIAL_COUNTER,
};

use crate::config::StrataXkbConfig;

pub enum KeyboardAction {
	ExecutedLua,
}

// complete list, for future reference
//
// Shift_L Shift_R
// Control_L Control_R
// Meta_L Meta_R
// Alt_L Alt_R
// Super_L Super_R
// Hyper_L Hyper_R
// ISO_Level2_Latch
// ISO_Level3_Shift ISO_Level3_Latch ISO_Level3_Lock
// ISO_Level5_Shift ISO_Level5_Latch ISO_Level5_Lock

// const KEY_Shift_L = 0xffe1;
// const KEY_Shift_R = 0xffe2;
// const KEY_Control_L = 0xffe3;
// const KEY_Control_R = 0xffe4;
// const KEY_Caps_Lock = 0xffe5;
// const KEY_Shift_Lock = 0xffe6;
//
// const KEY_Meta_L = 0xffe7;
// const KEY_Meta_R = 0xffe8;
// const KEY_Alt_L = 0xffe9;
// const KEY_Alt_R = 0xffea;
// const KEY_Super_L = 0xffeb;
// const KEY_Super_R = 0xffec;
// const KEY_Hyper_L = 0xffed;
// const KEY_Hyper_R = 0xffee;
//
//
// const KEY_ISO_Lock = 0xfe01;
// const KEY_ISO_Level2_Latch = 0xfe02;
// const KEY_ISO_Level3_Shift = 0xfe03;
// const KEY_ISO_Level3_Latch = 0xfe04;
// const KEY_ISO_Level3_Lock = 0xfe05;
// const KEY_ISO_Level5_Shift = 0xfe11;
// const KEY_ISO_Level5_Latch = 0xfe12;
// const KEY_ISO_Level5_Lock = 0xfe13;
// const KEY_ISO_Group_Shift = 0xff7e;
// const KEY_ISO_Group_Latch = 0xfe06;
// const KEY_ISO_Group_Lock = 0xfe07;
// const KEY_ISO_Next_Group = 0xfe08;
// const KEY_ISO_Next_Group_Lock = 0xfe09;
// const KEY_ISO_Prev_Group = 0xfe0a;
// const KEY_ISO_Prev_Group_Lock = 0xfe0b;
// const KEY_ISO_First_Group = 0xfe0c;
// const KEY_ISO_First_Group_Lock = 0xfe0d;
// const KEY_ISO_Last_Group = 0xfe0e;
// const KEY_ISO_Last_Group_Lock = 0xfe0f;
bitflags! {
	#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
	pub struct Modifier: u16 {
		const Shift_L = 1;
		const Shift_R = 1 << 1;
		const Control_L = 1 << 2;
		const Control_R = 1 << 3;
		const Alt_L = 1 << 4;
		const Alt_R = 1 << 5;
		const Super_L = 1 << 6;
		const Super_R = 1 << 7;
		const ISO_Level3_Shift = 1 << 8;
		const ISO_Level5_Shift = 1 << 9;
		const Hyper_L = 1 << 10;
		const Hyper_R = 1 << 11;
	}
}

impl<'gc> lua::FromValue<'gc> for Modifier {
	fn from_value(_: lua::Context<'gc>, value: lua::Value<'gc>) -> Result<Self, lua::TypeError> {
		match value {
			// lua::Value::Table(mods) => {
			// 	let mut r = Self::empty();
			//
			// 	for (_, v) in mods {
			// 		match v {
			// 			lua::Value::Integer(bits) => {
			// 				r |= Self::from_bits(bits as u16).ok_or(lua::TypeError {
			// 					expected: "Modifier (integer)",
			// 					found: "Invalid (integer)",
			// 				})?;
			// 			}
			// 			_ => {
			// 				return Err(lua::TypeError {
			// 					expected: "Modifier (integer)",
			// 					found: v.type_name(),
			// 				});
			// 			}
			// 		};
			// 	}
			//
			// 	Ok(r)
			// }
			lua::Value::Nil => Ok(Modifier::empty()),
			lua::Value::Integer(bits) => {
				Ok(Modifier::from_bits(bits as u16).ok_or(lua::TypeError {
					expected: "Modifier (integer)",
					found: "Invalid (integer)",
				})?)
			}
			_ => {
				Err(lua::TypeError {
					found: value.type_name(),
					expected: "table",
				})
			}
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key(Keysym);

impl From<Keysym> for Key {
	fn from(value: Keysym) -> Self {
		Self(value)
	}
}

impl<'gc> lua::FromValue<'gc> for Key {
	fn from_value(_: lua::Context<'gc>, value: lua::Value<'gc>) -> Result<Self, lua::TypeError> {
		match value {
			lua::Value::Integer(key) => Ok(Keysym::new(key as u32).into()),
			_ => {
				Err(lua::TypeError {
					found: value.type_name(),
					expected: "Key (integer)",
				})
			}
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPattern {
	pub modifier: Modifier,
	pub key: Key,
}

#[derive(Debug)]
pub struct Mods {
	pub flags: Modifier,
	pub state: ModifiersState,
}

impl super::Compositor {
	pub fn update_modifiers<I: InputBackend>(
		&mut self,
		new_modstate: &ModifiersState,
		keysym: Keysym,
		event: &I::KeyboardKeyEvent,
		keyboard: &KeyboardHandle<Self>,
	) {
		let old_modstate = self.mods.state;

		let modflag = match keysym {
			// equivalent to "Control_* + Shift_* + Alt_*" (on my keyboard *smile*)
			Keysym::Meta_L => Modifier::Alt_L,
			Keysym::Meta_R => Modifier::Alt_R,

			Keysym::Shift_L => Modifier::Shift_L,
			Keysym::Shift_R => Modifier::Shift_R,

			Keysym::Control_L => Modifier::Control_L,
			Keysym::Control_R => Modifier::Control_R,

			Keysym::Alt_L => Modifier::Alt_L,
			Keysym::Alt_R => Modifier::Alt_R,

			Keysym::Super_L => Modifier::Super_L,
			Keysym::Super_R => Modifier::Super_R,

			Keysym::ISO_Level3_Shift => Modifier::ISO_Level3_Shift,
			Keysym::ISO_Level5_Shift => Modifier::ISO_Level5_Shift,

			Keysym::Hyper_L => Modifier::Hyper_L,
			Keysym::Hyper_R => Modifier::Hyper_R,

			_ => Modifier::empty(),
		};

		match event.state() {
			KeyState::Pressed => {
				let depressed = if new_modstate == &old_modstate {
					// ignore previous modstate
					true
				} else {
					// "lock" key modifier or "normal" key modifier
					new_modstate.serialized.depressed > old_modstate.serialized.depressed
				};

				// "lock" key modifiers (Caps Lock, Num Lock, etc...) => `depressed` == `locked`
				// "normal" key modifiers (Control_*, Shift_*, etc...) => `depressed` > 0
				// "normal" keys (a, s, d, f) => `depressed` == 0
				let is_modifier =
					new_modstate.serialized.depressed > new_modstate.serialized.locked - old_modstate.serialized.locked;

				if is_modifier && depressed {
					self.mods.flags ^= modflag;
				}
			}
			KeyState::Released => {
				self.mods.flags ^= modflag;
			}
		};

		self.mods.state = *new_modstate;
	}

	fn on_keyboard<I: InputBackend>(
		&mut self,
		mods: &ModifiersState,
		keysymh: smithay::input::keyboard::KeysymHandle<'_>,
		event: <I as InputBackend>::KeyboardKeyEvent,
		rt: &mut super::Runtime,
		keyboard: &KeyboardHandle<Self>,
	) -> FilterResult<KeyboardAction> {
		self.update_modifiers::<I>(mods, keysymh.modified_sym(), &event, keyboard);

		// println!("{:#?}", comp.mods);
		// println!("{:#?}({:#?})", event.state(), keysym_h.modified_sym());
		match event.state() {
			KeyState::Pressed => {
				let k = KeyPattern {
					modifier: self.mods.flags,
					key: keysymh.modified_sym().into(),
				};

				let now = Instant::now();
				match rt.try_execute_closure::<(), _>(self, |ctx, comp| {
					comp.config
						.input_config
						.global_keybinds
						.get(&k)
						.map(|cb| (ctx.fetch(cb), []))
				}) {
					Some(r) => {
						println!("elapsed: {:?}", now.elapsed());
						if let Err(e) = r {
							println!("{:?}", e);
						}
						FilterResult::Intercept(KeyboardAction::ExecutedLua)
					}
					None => FilterResult::Forward,
				}
			}
			KeyState::Released => FilterResult::Forward,
		}
	}

	pub fn update_xkbconfig(&mut self, cfg: &StrataXkbConfig) -> anyhow::Result<()> {
		let keyboard = self.seat.get_keyboard().context("Unable to get keyboard handle")?;
		keyboard
			.set_xkb_config(
				self,
				XkbConfig {
					layout: &cfg.layout,
					rules: &cfg.rules,
					model: &cfg.model,
					options: cfg.options.clone(),
					variant: &cfg.variant,
				},
			)
			.with_context(|| format!("Invalid config: {:?}", cfg))?;
		self.mods.state = keyboard.modifier_state();

		Ok(())
	}
}

impl super::Strata {
	pub fn on_keyboard<I: InputBackend>(&mut self, event: I::KeyboardKeyEvent) -> anyhow::Result<()> {
		let serial = SERIAL_COUNTER.next_serial();
		let time = Event::time_msec(&event);

		let keyboard = self.comp.seat.get_keyboard().context("no keyboard attached to seat")?;

		if let Some(action) = keyboard.input(
			&mut self.comp,
			event.key_code(),
			event.state(),
			serial,
			time,
			|comp, mods, keysymh| comp.on_keyboard::<I>(mods, keysymh, event, &mut self.rt, &keyboard),
		) {
			match action {
				KeyboardAction::ExecutedLua => {}
			}
		};

		Ok(())
	}
}
