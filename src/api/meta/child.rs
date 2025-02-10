use std::{
	io::Read,
	ops,
	os::fd::AsFd,
	process,
	time::Instant,
};

use anyhow::Context as _;
use gc_arena::{
	Collect,
	RefLock,
	Rootable,
	barrier::Write,
};
use nix::unistd::Pid;
use piccolo::{
	self as lua,
	IntoValue,
};

use crate::{
	api::ContextExt as _,
	util::ReadLineCb,
};

#[derive(Collect)]
#[collect(no_drop)]
pub struct Child(#[collect(require_static)] process::Child);

impl ops::Deref for Child {
	type Target = process::Child;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl ops::DerefMut for Child {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}

fn register_read_line_cb<'gc, S>(
	ctx: lua::Context<'gc>,
	comp: lua::UserData<'gc>,
	s: Option<S>,
	cb: lua::Function<'gc>,
) -> anyhow::Result<()>
where
	S: 'static + Read + AsFd,
{
	let Some(src) = s else {
		return Ok(());
	};

	let src = ReadLineCb::new(src, ctx.stash(cb))?;

	let fcomp = ctx.fcomp(comp)?;
	fcomp.with(|comp| {
		let reg = comp
			.loop_handle
			.insert_source(src, move |_, m, strata| {
				let now = Instant::now();
				if let Err(e) = strata.execute_closure::<(), _>(|ctx, _| {
					let s = lua::String::from_slice(&ctx, &m.buf[..m.buf.len() - 1]).into_value(ctx);

					(ctx.fetch(&m.cb), [s])
				}) {
					println!("{:?}", e);
				};
				println!("elapsed: {:?}", now.elapsed());

				Ok(())
			})
			.map_err(|e| e.error)?;

		anyhow::Ok(())
	})?;

	Ok(())
}

impl Child {
	pub fn new_userdata<'gc>(
		ctx: lua::Context<'gc>,
		comp: lua::UserData<'gc>,
		child: process::Child,
	) -> anyhow::Result<lua::UserData<'gc>> {
		let ud = lua::UserData::new::<Rootable![RefLock<Self>]>(&ctx, RefLock::new(Self(child)));
		ud.set_metatable(&ctx, Some(Self::meta(ctx, comp)?));
		Ok(ud)
	}

	fn from_userdata<'gc>(ctx: lua::Context<'gc>, ud: lua::UserData<'gc>) -> anyhow::Result<&'gc Write<RefLock<Self>>> {
		ud.downcast_write::<Rootable![RefLock<Self>]>(&ctx)
			.context("expected `Child (userdata)`, got `Unknown (userdata)`")
	}

	fn meta<'gc>(ctx: lua::Context<'gc>, comp: lua::UserData<'gc>) -> anyhow::Result<lua::Table<'gc>> {
		let index = lua::Table::new(&ctx);

		index.set(
			ctx,
			"on_line_stdout",
			lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
				let (ud, cb) = stack.consume::<(lua::UserData, lua::Function)>(ctx)?;
				stack.replace(ctx, ud);

				let this = Self::from_userdata(ctx, ud)?;
				register_read_line_cb(ctx, comp, this.unlock().borrow_mut().stdout.take(), cb)?;

				Ok(lua::CallbackReturn::Return)
			}),
		)?;

		index.set(
			ctx,
			"on_line_stderr",
			lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
				let (ud, cb) = stack.consume::<(lua::UserData, lua::Function)>(ctx)?;
				stack.replace(ctx, ud);

				let this = Self::from_userdata(ctx, ud)?;
				register_read_line_cb(ctx, comp, this.unlock().borrow_mut().stderr.take(), cb)?;

				Ok(lua::CallbackReturn::Return)
			}),
		)?;

		index.set(
			ctx,
			"on_exit",
			lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
				let (ud, cb) = stack.consume::<(lua::UserData, lua::Function)>(ctx)?;
				stack.replace(ctx, ud);

				let this = Self::from_userdata(ctx, ud)?;
				let pid = this.borrow().id();
				println!("pid={}", pid);

				let fcomp = ctx.fcomp(comp)?;
				fcomp.with_mut(|comp| {
					comp.process_state
						.on_exit_cbs
						.insert(Pid::from_raw(pid as i32), ctx.stash(cb));
				});

				Ok(lua::CallbackReturn::Return)
			}),
		)?;

		index.set(
			ctx,
			"wait",
			lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
				let (ud, cb) = stack.consume::<(lua::UserData, lua::Function)>(ctx)?;

				let this = Self::from_userdata(ctx, ud)?;

				let status = this.unlock().borrow_mut().wait()?;

				stack.replace(ctx, status.code());

				Ok(lua::CallbackReturn::Return)
			}),
		)?;

		index.set(
			ctx,
			"kill",
			lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
				let (ud,) = stack.consume::<(lua::UserData,)>(ctx)?;

				let this = Self::from_userdata(ctx, ud)?;

				this.unlock().borrow_mut().kill()?;
				// this.unlock().borrow_mut().wait()?;

				Ok(lua::CallbackReturn::Return)
			}),
		)?;

		let meta = lua::Table::new(&ctx);
		meta.set(ctx, lua::MetaMethod::Index, index)?;
		Ok(meta)
	}
}
