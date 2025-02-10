use anyhow::Context as _;
use piccolo::{
	self as lua,
	IntoValue as _,
};

use super::ContextExt;

pub fn api<'gc>(ctx: lua::Context<'gc>, comp: lua::UserData<'gc>) -> anyhow::Result<lua::Value<'gc>> {
	let window = lua::Table::new(&ctx);

	window.set(
		ctx,
		"foo",
		lua::Callback::from_fn_with(&ctx, comp, |&comp, ctx, _, mut stack| {
			let (cb,) = stack.consume::<(lua::Function,)>(ctx)?;

			let seq = lua::async_sequence(&ctx, |locals, mut seq| {
				let comp = locals.stash(&ctx, comp);
				let cb = locals.stash(&ctx, cb);

				async move {
					seq.enter(|ctx, _, _, mut stack| {
						//
					});
					seq.call(&cb, 0).await?;
					Ok(lua::SequenceReturn::Return)
				}
			});

			Ok(lua::CallbackReturn::Sequence(seq))
		}),
	)?;

	Ok(window.into_value(ctx))
}
