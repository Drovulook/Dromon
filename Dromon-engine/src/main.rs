use crate::app::App;
use anyhow::Result;
use winit::event_loop::{ControlFlow, EventLoop};

mod app;

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let use_cli = std::env::args().any(|a| a == "--use-cli");

    let mut app = App::new(use_cli);
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut app)?;

    Ok(())
}
