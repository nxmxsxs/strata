use piccolo::{
	self as lua,
};

use crate::{
	state::{Compositor, input},
	util::FxIndexMap,
};

mod from_lua;
mod parse;
mod structs;

// pub use parse::parse_config;
// pub use structs::*;

#[derive(Debug)]
pub struct StrataRepeatInfoConfig {
	pub rate: i32,
	pub delay: i32,
}

impl Default for StrataRepeatInfoConfig {
	fn default() -> Self {
		Self {
			rate: 20,
			delay: 200,
		}
	}
}

#[derive(Debug, Default)]
pub struct StrataXkbConfig {
	pub layout: String,
	pub rules: String,
	pub model: String,
	pub options: Option<String>,
	pub variant: String,
}

impl StrataXkbConfig {
	pub fn update<F>(comp: &mut Compositor, mut f: F) -> anyhow::Result<()>
	where
		F: FnMut(Option<&mut Self>) -> anyhow::Result<()>,
	{
		let mut cfg = comp.config.input_config.xkbconfig.take();
		f(cfg.as_mut())?;

		if let Some(cfg) = cfg.as_ref() {
			comp.update_xkbconfig(cfg)?;
		}
		comp.config.input_config.xkbconfig = cfg;

		Ok(())
	}
}

#[derive(Debug)]
pub struct StrataInputConfig {
	pub repeat_info: StrataRepeatInfoConfig,
	pub xkbconfig: Option<StrataXkbConfig>,
	pub global_keybinds: FxIndexMap<input::KeyPattern, lua::StashedFunction>,
	pub global_mousebinds: FxIndexMap<input::KeyPattern, lua::StashedFunction>,
}

impl Default for StrataInputConfig {
	fn default() -> Self {
		Self {
			repeat_info: Default::default(),
			xkbconfig: Some(StrataXkbConfig {
				layout: String::from("it"),
				rules: String::new(),
				model: String::new(),
				options: Some(String::from("caps:swapescape")),
				variant: String::new(),
			}),
			global_keybinds: Default::default(),
			global_mousebinds: Default::default(),
		}
	}
}

#[derive(Debug, Default)]
pub struct StrataConfig {
	pub input_config: StrataInputConfig,
}
