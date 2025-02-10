use nix::{
	sys::{
		signal,
		wait::{
			WaitStatus,
			waitpid,
		},
	},
	unistd::Pid,
};
use once_cell::sync::OnceCell;
use piccolo::{
	self as lua,
	IntoValue,
};
use smithay::reexports::calloop;

use crate::{
	state::Strata,
	util::FxIndexMap,
};

pub static CHLDTX: OnceCell<calloop::channel::Sender<WaitStatus>> = OnceCell::new();

pub fn init_sigchld_handler() -> anyhow::Result<()> {
	unsafe {
		extern "C" fn handler(signal: i32) {
			// Reap any child process that has exited
			loop {
				match waitpid(None, Some(nix::sys::wait::WaitPidFlag::WNOHANG)) {
					Ok(ws @ WaitStatus::Exited(pid, status)) => {
						if let Some(c) = CHLDTX.get() {
							if let Err(e) = c.send(ws) {
								println!("{:?}", e);
							}
						};
						println!("Child process with PID {} exited with status {}", pid, status);
					}
					Ok(ws @ WaitStatus::Signaled(pid, signal, _)) => {
						if let Some(c) = CHLDTX.get() {
							if let Err(e) = c.send(ws) {
								println!("{:?}", e);
							}
						};
						println!("Child process with PID {} was killed by signal {}", pid, signal);
					}
					Ok(WaitStatus::Stopped(pid, signal)) => {
						println!("Child process with PID {} was stopped by signal {}", pid, signal);
					}
					_ => break,
				}
			}
		}
		signal::signal(signal::Signal::SIGCHLD, signal::SigHandler::Handler(handler))?;
	}

	Ok(())
}

pub struct ProcessState {
	pub on_exit_cbs: FxIndexMap<Pid, lua::StashedFunction>,
}

impl ProcessState {
	pub fn new(loop_handle: &calloop::LoopHandle<'static, super::Strata>) -> anyhow::Result<Self> {
		let (chldtx, chldrx) = calloop::channel::channel();

		if let Err(_chldtx) = CHLDTX.set(chldtx) {
			println!("unable to set CHLDTX global");
		};

		fn call_exit_cb<const N: usize>(strata: &mut Strata, pid: Pid, args: [impl for<'gc> lua::IntoValue<'gc>; N]) {
			if let Some(Err(e)) = strata.try_execute_closure::<(), N>(|ctx, comp| {
				comp.process_state.on_exit_cbs.get(&pid).map(|cb| {
					let args = args.map(|v| v.into_value(ctx));

					(ctx.fetch(cb), args)
				})
			}) {
				println!("{:?}", e);
			}
		}

		loop_handle
			.insert_source(chldrx, |event, _, strata| {
				match event {
					calloop::channel::Event::Msg(ws) => {
						match ws {
							WaitStatus::Exited(pid, code) => {
								call_exit_cb(strata, pid, [code as i64, 0]);
							}
							WaitStatus::Signaled(pid, signal, _) => {
								call_exit_cb(strata, pid, [0, signal as i64]);
							}

							// WaitStatus::Stopped(pid, signal) => todo!(),
							// WaitStatus::PtraceEvent(pid, signal, _) => todo!(),
							// WaitStatus::PtraceSyscall(pid) => todo!(),
							// WaitStatus::Continued(pid) => todo!(),
							// WaitStatus::StillAlive => todo!(),
							_ => unreachable!(),
						}
					}
					calloop::channel::Event::Closed => {}
				}
				//
			})
			.map_err(|e| e.error)?;

		Ok(Self {
			on_exit_cbs: FxIndexMap::default(),
		})
	}
}
