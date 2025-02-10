// Copyright 2023 the Strata authors
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::Context as _;
use piccolo::{
	self as lua,
	FromValue as _,
};
use smithay::input::keyboard::xkb;

use crate::{
	api::ContextExt,
	config::StrataXkbConfig,
	state::input,
	util::get_str_from_value,
};

fn key<'gc>(ctx: lua::Context<'gc>, comp: lua::UserData<'gc>) -> anyhow::Result<lua::Table<'gc>> {
	let meta = lua::Table::new(&ctx);

	meta.set(
		ctx,
		lua::MetaMethod::Index,
		lua::Callback::from_fn(&ctx, |ctx, _, mut stack| {
			let (this, k) = stack.consume::<(lua::Table, lua::String)>(ctx)?;

			let ksym = xkb::keysym_from_name(k.to_str()?, xkb::KEYSYM_CASE_INSENSITIVE);
			let v = lua::Value::Integer(ksym.raw() as i64);
			this.set_raw(&ctx, k.into(), v)?;
			// println!("k = {:?}", k);

			stack.push_front(v);

			Ok(lua::CallbackReturn::Return)
		}),
	)?;

	meta.set(
		ctx,
		lua::MetaMethod::NewIndex,
		lua::Callback::from_fn(&ctx, |_, _, _| Ok(lua::CallbackReturn::Return)),
	)?;

	let keys = lua::Table::new(&ctx);
	keys.set_metatable(&ctx, Some(meta));

	Ok(keys)
}

fn modifier<'gc>(ctx: lua::Context<'gc>, comp: lua::UserData<'gc>) -> anyhow::Result<lua::Table<'gc>> {
	let meta = lua::Table::new(&ctx);

	meta.set(
		ctx,
		lua::MetaMethod::Index,
		lua::Callback::from_fn(&ctx, |ctx, _, mut stack| {
			let (_, k) = stack.consume::<(lua::Table, lua::String)>(ctx)?;
			let k = k.to_str()?;
			let bits = input::Modifier::from_name(k).with_context(|| format!("Invalid Modifier: {}", k))?;

			stack.push_front(lua::Value::Integer(bits.bits() as i64));

			Ok(lua::CallbackReturn::Return)
		}),
	)?;

	let modifiers = lua::Table::new(&ctx);
	modifiers.set_metatable(&ctx, Some(meta));

	Ok(modifiers)
}

pub fn api<'gc>(ctx: lua::Context<'gc>, comp: lua::UserData<'gc>) -> anyhow::Result<lua::Value<'gc>> {
	let input = lua::Table::new(&ctx);

	input.set_field(ctx, "Key", key(ctx, comp)?);
	input.set_field(ctx, "Modifier", modifier(ctx, comp)?);

	input.set_field(
		ctx,
		"keybind",
		lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
			let fcomp = ctx.fcomp(comp)?;
			let (modifier, key, cb) = stack.consume::<(input::Modifier, input::Key, lua::Function)>(ctx)?;

			let keypat = input::KeyPattern {
				modifier,
				key,
			};

			fcomp.with_mut(|comp| {
				comp.config.input_config.global_keybinds.insert(keypat, ctx.stash(cb));
			});

			Ok(lua::CallbackReturn::Return)
		}),
	);

	input.set_field(
		ctx,
		"setup",
		lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
			let fcomp = ctx.fcomp(comp)?;
			let (cfg,) = stack.consume::<(lua::Table,)>(ctx)?;

			fcomp.with_mut(|comp| {
				if let Some(repeat_info) = Option::<lua::Table>::from_value(ctx, cfg.get_value(ctx, "repeat_info"))
					.context("`repeat_info` is invalid")?
				{
					let rate = i32::from_value(ctx, repeat_info.get_value(ctx, "rate"))
						.unwrap_or(comp.config.input_config.repeat_info.rate);
					let delay = i32::from_value(ctx, repeat_info.get_value(ctx, "delay"))
						.unwrap_or(comp.config.input_config.repeat_info.delay);

					comp.seat
						.get_keyboard()
						.context("Unable to get keyboard")?
						.change_repeat_info(rate.abs(), delay.abs());
				}

				if let Some(xkbconfig) = Option::<lua::Table>::from_value(ctx, cfg.get_value(ctx, "xkbconfig"))
					.context("`xkbconfig` is invalid")?
				{
					StrataXkbConfig::update(comp, |cfg| {
						let Some(cfg) = cfg else { return Ok(()) };

						if let Some(layout) = Option::<lua::String>::from_value(ctx, xkbconfig.get_value(ctx, "layout"))
							.context("`xkbconfig.layout` is invalid")?
							.map(|s| get_str_from_value(ctx, s.into()))
							.and_then(|s| s.ok())
						{
							cfg.layout.replace_range(.., layout);
						}

						if let Some(rules) = Option::<lua::String>::from_value(ctx, xkbconfig.get_value(ctx, "rules"))
							.context("`xkbconfig.rules` is invalid")?
							.map(|s| get_str_from_value(ctx, s.into()))
							.and_then(|s| s.ok())
						{
							cfg.rules.replace_range(.., rules);
						}

						if let Some(model) = Option::<lua::String>::from_value(ctx, xkbconfig.get_value(ctx, "model"))
							.context("`xkbconfig.model` is invalid")?
							.map(|s| get_str_from_value(ctx, s.into()))
							.and_then(|s| s.ok())
						{
							cfg.model.replace_range(.., model);
						}

						if let Some(options) =
							Option::<lua::String>::from_value(ctx, xkbconfig.get_value(ctx, "options"))
								.context("`xkbconfig.options` is invalid")?
								.map(|s| get_str_from_value(ctx, s.into()))
								.and_then(|s| s.ok())
						{
							if let Some(s) = cfg.options.as_mut() {
								s.replace_range(.., options);
							} else {
								cfg.options = Some(options.to_string());
							}
						}

						if let Some(variant) =
							Option::<lua::String>::from_value(ctx, xkbconfig.get_value(ctx, "variant"))
								.context("`xkbconfig.variant` is invalid")?
								.map(|s| get_str_from_value(ctx, s.into()))
								.and_then(|s| s.ok())
						{
							cfg.variant.replace_range(.., variant);
						}

						Ok(())
					})?;
				}

				// if let lua::Value::Table(keybinds) = cfg.get_value(ctx, "keybinds") {
				// 	for (_, keybind) in keybinds {
				// 		match keybind {
				// 			lua::Value::Table(keybind) => {
				// 				let modifiers = Modifiers::from_value(ctx, keybind.get_value(ctx, 1))?;
				// 				let key = Key::from_value(ctx, keybind.get_value(ctx, 2))?;
				// 				let cb = lua::Function::from_value(ctx, keybind.get_value(ctx, 3))?;
				//
				// 				comp.config.input_config.global_keybinds.insert(
				// 					KeyPattern {
				// 						modifiers,
				// 						key,
				// 					},
				// 					ctx.stash(cb),
				// 				);
				// 			}
				// 			_ => {
				// 				todo!()
				// 			}
				// 		}
				// 	}
				// }

				anyhow::Ok(())
			})?;

			Ok(lua::CallbackReturn::Return)
		}),
	);

	Ok(input.into())
}
