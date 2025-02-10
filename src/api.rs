// Copyright 2023 the Strata authors
// SPDX-License-Identifier: GPL-3.-or-later

use anyhow::Context as _;
use piccolo::{
	self as lua,
};

use crate::state::FrozenCompositor;

mod input;
mod meta;
mod proc;
mod window;

trait ContextExt<'gc> {
	fn fcomp(self, ud: lua::UserData<'gc>) -> anyhow::Result<&'gc FrozenCompositor>;
}

impl<'gc> ContextExt<'gc> for lua::Context<'gc> {
	fn fcomp(self, ud: lua::UserData<'gc>) -> anyhow::Result<&'gc FrozenCompositor> {
		ud.downcast_static::<FrozenCompositor>()
			.context("expected `FrozenCompositor (userdata)`, got `Unknown (userdata)`")
	}
}

pub fn create_global(ctx: lua::Context<'_>, fcomp: FrozenCompositor) -> anyhow::Result<lua::UserData<'_>> {
	let comp = lua::UserData::new_static(&ctx, fcomp);

	let index = lua::Table::new(&ctx);

	index.set(ctx, "input", input::api(ctx, comp)?)?;
	index.set(ctx, "proc", proc::api(ctx, comp)?)?;
	index.set(ctx, "window", window::api(ctx, comp)?)?;
	index.set(
		ctx,
		"quit",
		lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, _| {
			let fcomp = ctx.fcomp(comp)?;

			fcomp.with(|comp| {
				comp.quit();
			});

			Ok(lua::CallbackReturn::Return)
		}),
	)?;

	let meta = lua::Table::new(&ctx);
	meta.set(ctx, lua::MetaMethod::Index, index)?;
	comp.set_metatable(&ctx, Some(meta));

	Ok(comp)
}
