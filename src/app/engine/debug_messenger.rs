use anyhow::Result;
use ash::vk;
use std::ffi::CStr;

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _type: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let message = unsafe { CStr::from_ptr((*data).p_message) }
        .to_str()
        .unwrap_or("?");
    eprintln!("[Vulkan {:?}] {}\n", severity, message);
    vk::FALSE
}

pub struct DebugMessenger {
    handle: vk::DebugUtilsMessengerEXT,
    loader: ash::ext::debug_utils::Instance,
}

impl DebugMessenger {
    pub fn new(entry: &ash::Entry, instance: &ash::Instance) -> Result<Self> {
        unsafe {
            let loader = ash::ext::debug_utils::Instance::new(entry, instance);
            let handle = loader.create_debug_utils_messenger(
                &vk::DebugUtilsMessengerCreateInfoEXT::default()
                    .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                    )
                    .message_type(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION)
                    .pfn_user_callback(Some(debug_callback)),
                None,
            )?;
            Ok(Self { handle, loader })
        }
    }

    pub unsafe fn destroy(&self) {
        self.loader.destroy_debug_utils_messenger(self.handle, None);
    }
}
