use anyhow::Result;
use ash::vk;
use std::ffi::CStr;
use std::sync::mpsc::SyncSender;

unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _type: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let message = unsafe { CStr::from_ptr((*data).p_message) }
        .to_str()
        .unwrap_or("?");
    let formatted = format!("[Vulkan {:?}] {}", severity, message);

    if !user_data.is_null() {
        // Safety: user_data pointe vers le SyncSender<String> owned par DebugMessenger,
        // qui vit aussi longtemps que ce callback peut être appelé.
        let sender = unsafe { &*(user_data as *const SyncSender<String>) };
        let _ = sender.try_send(formatted);
    } else {
        eprintln!("{}", formatted);
    }

    vk::FALSE
}

pub struct DebugMessenger {
    handle: vk::DebugUtilsMessengerEXT,
    loader: ash::ext::debug_utils::Instance,
    _sender: Option<Box<SyncSender<String>>>,
}

impl DebugMessenger {
    pub fn new(
        entry: &ash::Entry,
        instance: &ash::Instance,
        sender: Option<SyncSender<String>>,
    ) -> Result<Self> {
        let sender = sender.map(Box::new);
        let user_data = sender
            .as_deref()
            .map(|s| s as *const SyncSender<String> as *mut std::ffi::c_void)
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
            Ok(Self { handle, loader, _sender: sender })
        }
    }

    pub unsafe fn destroy(&self) {
        unsafe {
            self.loader.destroy_debug_utils_messenger(self.handle, None);
        }
    }
}
