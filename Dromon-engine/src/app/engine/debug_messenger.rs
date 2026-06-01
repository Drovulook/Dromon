use anyhow::Result;
use ash::vk;
use std::ffi::CStr;
use std::sync::Arc;

use crate::app::logger::Logger;

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _type: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let message = unsafe { CStr::from_ptr((*data).p_message) }
        .to_str()
        .unwrap_or("?");
    let formatted = format!("[{:?}] {}", severity, message);

    if !user_data.is_null() {
        // Safety: user_data pointe vers le Logger heap-alloué par l'Arc,
        // qui vit aussi longtemps que DebugMessenger (via _logger).
        let logger = unsafe { &*(user_data as *const Logger) };
        logger.vulkan_layer_msg(&formatted);
    } else {
        eprintln!("{}", formatted);
    }

    vk::FALSE
}

pub struct DebugMessenger {
    handle: vk::DebugUtilsMessengerEXT,
    loader: ash::ext::debug_utils::Instance,
    _logger: Option<Arc<Logger>>,
}

impl DebugMessenger {
    pub fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        logger: Option<Arc<Logger>>,
    ) -> Result<Self> {
        // On pointe directement dans l'allocation heap de l'Arc (stable tant que l'Arc vit).
        let user_data = logger
            .as_ref()
            .map(|arc| arc.as_ref() as *const Logger as *mut std::ffi::c_void)
            .unwrap_or(std::ptr::null_mut());

        unsafe {
            let loader = ash::ext::debug_utils::Instance::new(entry, instance);
            let mut create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION)
                .pfn_user_callback(Some(debug_callback));
            create_info.p_user_data = user_data;

            let handle = loader.create_debug_utils_messenger(&create_info, None)?;
            Ok(Self {
                handle,
                loader,
                _logger: logger,
            })
        }
    }

    pub unsafe fn destroy(&self) {
        unsafe {
            self.loader.destroy_debug_utils_messenger(self.handle, None);
        }
    }
}
