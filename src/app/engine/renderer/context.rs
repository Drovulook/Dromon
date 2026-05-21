use anyhow::Result;
use std::sync::Arc;
use winit::window::Window;

pub struct Context {}

impl Context {
    pub fn new(window: Arc<Window>) -> Result<Self> {
        unsafe {
            let entry = ash::Entry::load()?;
            let instance = entry.create_instance(&vk::InstanceCreateInfo::default(), None)?;
        }
        Ok(Self {})
    }
}
