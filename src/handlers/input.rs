// Copyright 2023 the Strata authors
// SPDX-License-Identifier: GPL-3.0-or-later

use smithay::{
	backend::input::{
		AbsolutePositionEvent,
		Axis,
		AxisSource,
		Event,
		InputBackend,
		PointerAxisEvent,
		PointerButtonEvent,
		PointerMotionEvent,
	},
	input::pointer::{
		AxisFrame,
		ButtonEvent,
		MotionEvent,
		RelativeMotionEvent,
	},
	utils::SERIAL_COUNTER,
};

use crate::{
	state::Compositor,
	workspaces::FocusTarget,
};

impl Compositor {
	pub fn set_input_focus(&mut self, target: FocusTarget) {
		let keyboard = self.seat.get_keyboard().unwrap();
		let serial = SERIAL_COUNTER.next_serial();
		keyboard.set_focus(self, Some(target), serial);
	}

	pub fn set_input_focus_auto(&mut self) {
		let under = self.surface_under();
		if let Some(d) = under {
			self.set_input_focus(d.0);
		}
	}

	pub fn pointer_motion<I: InputBackend>(&mut self, event: I::PointerMotionEvent) -> anyhow::Result<()> {
		let serial = SERIAL_COUNTER.next_serial();
		let delta = (event.delta_x(), event.delta_y()).into();

		self.set_input_focus_auto();

		if let Some(ptr) = self.seat.get_pointer() {
			let location = self.workspaces.current().clamp_coords(ptr.current_location() + delta);

			let under = self.surface_under();

			ptr.motion(
				self,
				under.clone(),
				&MotionEvent {
					location,
					serial,
					time: event.time_msec(),
				},
			);

			ptr.relative_motion(
				self,
				under,
				&RelativeMotionEvent {
					delta,
					delta_unaccel: event.delta_unaccel(),
					utime: event.time(),
				},
			)
		}

		Ok(())
	}

	pub fn pointer_motion_absolute<I: InputBackend>(
		&mut self,
		event: I::PointerMotionAbsoluteEvent,
	) -> anyhow::Result<()> {
		let serial = SERIAL_COUNTER.next_serial();

		let curr_workspace = self.workspaces.current();
		let output = curr_workspace.outputs().next().unwrap();
		let output_geo = curr_workspace.output_geometry(output).unwrap();
		let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

		let location = self.workspaces.current().clamp_coords(pos);

		self.set_input_focus_auto();

		let under = self.surface_under();
		if let Some(ptr) = self.seat.get_pointer() {
			ptr.motion(
				self,
				under,
				&MotionEvent {
					location,
					serial,
					time: event.time_msec(),
				},
			);
		}

		Ok(())
	}

	pub fn pointer_button<I: InputBackend>(&mut self, event: I::PointerButtonEvent) -> anyhow::Result<()> {
		let serial = SERIAL_COUNTER.next_serial();

		let button = event.button_code();
		let button_state = event.state();
		self.set_input_focus_auto();
		if let Some(ptr) = self.seat.get_pointer() {
			ptr.button(
				self,
				&ButtonEvent {
					button,
					state: button_state,
					serial,
					time: event.time_msec(),
				},
			);
		}

		Ok(())
	}

	pub fn pointer_axis<I: InputBackend>(&mut self, event: I::PointerAxisEvent) -> anyhow::Result<()> {
		let horizontal_amount = event
			.amount(Axis::Horizontal)
			.unwrap_or_else(|| event.amount(Axis::Horizontal).unwrap_or(0.0) * 3.0);
		let vertical_amount = event
			.amount(Axis::Vertical)
			.unwrap_or_else(|| event.amount(Axis::Vertical).unwrap_or(0.0) * 3.0);

		let mut frame = AxisFrame::new(event.time_msec()).source(event.source());
		if horizontal_amount != 0.0 {
			frame = frame.value(Axis::Horizontal, horizontal_amount);
		} else if event.source() == AxisSource::Finger {
			frame = frame.stop(Axis::Horizontal);
		}
		if vertical_amount != 0.0 {
			frame = frame.value(Axis::Vertical, vertical_amount);
		} else if event.source() == AxisSource::Finger {
			frame = frame.stop(Axis::Vertical);
		}

		if let Some(ptr) = self.seat.get_pointer() {
			ptr.axis(self, frame);
		}

		Ok(())
	}
}
