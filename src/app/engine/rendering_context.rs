/* Assumptions:
 * - Vulkan version 1.3
 * - GPU discret (DISCRETE_GPU) requis — single_queue_family rejette les iGPU
 * - Une seule queue créée par famille (queue_priorities: &[1.0])
 * - graphics / present / transfer / compute sur la même famille (single_queue_family)
 * - Surface support vérifiée sur queue index 0 uniquement (retain hardcodé)
 * - Features supportées par le GPU : dynamic_rendering, buffer_device_address
 * - Extension VK_KHR_swapchain disponible
 * - La compatibility_surface est temporaire : détruite après filtrage des devices
 */

use super::debug_messenger::DebugMessenger;
use anyhow::Result;
use ash::vk;
use std::ffi::CStr;
use std::{collections::HashSet, sync::Arc};
use winit::{
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::Window,
};

pub struct RenderingContext {
    pub queues: Vec<vk::Queue>,
    pub swapchain_extensions: ash::khr::swapchain::Device,
    pub device: ash::Device,
    pub queue_indices_set: HashSet<u32>,
    pub queue_families: QueueFamilies,
    pub physical_device: PhysicalDevice,
    pub debug_messenger: Option<DebugMessenger>,
    pub surface_extensions: ash::khr::surface::Instance,
    pub instance: ash::Instance,
    pub entry: ash::Entry,
}

type QueueFamilyPicker = fn(Vec<PhysicalDevice>) -> Result<(PhysicalDevice, QueueFamilies)>;

pub struct ContextAttributes<'window> {
    pub compatibility_window: &'window Window,
    pub queue_family_picker: QueueFamilyPicker,
}

#[derive(Debug, Clone)]
pub struct PhysicalDevice {
    pub handle: vk::PhysicalDevice,
    pub properties: vk::PhysicalDeviceProperties,
    pub features: vk::PhysicalDeviceFeatures,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub queue_families: Vec<QueueFamily>,
}

#[derive(Debug, Clone)]
pub struct QueueFamily {
    pub index: u32,
    pub properties: vk::QueueFamilyProperties,
}

pub struct QueueFamilies {
    pub graphics: u32,
    pub present: u32,
    pub transfer: u32,
    pub compute: u32,
}

pub mod queue_family_picker {
    use crate::app::engine::rendering_context::{PhysicalDevice, QueueFamilies};
    use anyhow::Context as AnyhowContext;
    use anyhow::Result;
    use ash::vk;

    pub fn single_queue_family(
        physical_devices: Vec<PhysicalDevice>,
    ) -> Result<(PhysicalDevice, QueueFamilies)> {
        let physical_device = physical_devices
            .into_iter()
            .find(|handle| handle.properties.device_type == vk::PhysicalDeviceType::DISCRETE_GPU)
            .context("Aucun GPU discret trouvé")?;

        let queue_family = physical_device
            .queue_families
            .iter()
            .find(|family| {
                family
                    .properties
                    .queue_flags
                    .contains(vk::QueueFlags::GRAPHICS)
                    && family
                        .properties
                        .queue_flags
                        .contains(vk::QueueFlags::COMPUTE)
            })
            .map(|family| family.index)
            .context("Aucune queue GRAPHICS+COMPUTE trouvée")?;

        Ok((
            physical_device,
            QueueFamilies {
                graphics: queue_family,
                present: queue_family,
                transfer: queue_family,
                compute: queue_family,
            },
        ))
    }
}

const VALIDATION_LAYER: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(b"VK_LAYER_KHRONOS_validation\0") };
const DEBUG_UTILS_EXT: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(b"VK_EXT_debug_utils\0") };

impl RenderingContext {
    pub fn new(attributes: ContextAttributes) -> Result<Self> {
        unsafe {
            // TODO: créer entry et instance une seule fois, pas une fois / renderer
            let entry = ash::Entry::load()?;
            let raw_display_handle = attributes.compatibility_window.display_handle()?.as_raw();
            let raw_window_handle = attributes.compatibility_window.window_handle()?.as_raw();

            // validation layers
            let mut extensions =
                ash_window::enumerate_required_extensions(raw_display_handle)?.to_vec();
            #[cfg(debug_assertions)]
            extensions.push(DEBUG_UTILS_EXT.as_ptr());

            let layers = {
                #[cfg(debug_assertions)]
                {
                    vec![VALIDATION_LAYER.as_ptr()]
                }
                #[cfg(not(debug_assertions))]
                {
                    vec![]
                }
            };

            let debug_messenger = None::<DebugMessenger>; //

            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(
                        &vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3),
                    )
                    .enabled_extension_names(&extensions)
                    .enabled_layer_names(&layers),
                None,
            )?;

            #[cfg(debug_assertions)]
            let debug_messenger = Some(DebugMessenger::new(&entry, &instance)?);

            let surface_extensions = ash::khr::surface::Instance::new(&entry, &instance);

            let compatibility_surface = ash_window::create_surface(
                &entry,
                &instance,
                raw_display_handle,
                raw_window_handle,
                None,
            )?;

            let mut physical_devices = instance
                .enumerate_physical_devices()?
                .into_iter()
                .map(|device| {
                    let properties = instance.get_physical_device_properties(device);
                    let features = instance.get_physical_device_features(device);
                    let memory_properties = instance.get_physical_device_memory_properties(device);
                    let queue_family_properties =
                        instance.get_physical_device_queue_family_properties(device);

                    let queue_families = queue_family_properties
                        .into_iter()
                        .enumerate()
                        .map(|(index, properties)| QueueFamily {
                            index: index as u32,
                            properties,
                        })
                        .collect::<Vec<_>>();

                    PhysicalDevice {
                        handle: device,
                        properties,
                        features,
                        memory_properties,
                        queue_families,
                    }
                })
                .collect::<Vec<_>>();
            // println!("Physical devices: {:#?}", physical_devices);

            physical_devices.retain(|device| {
                surface_extensions
                    .get_physical_device_surface_support(device.handle, 0, compatibility_surface)
                    .unwrap_or(false)
            });
            surface_extensions.destroy_surface(compatibility_surface, None);

            let (physical_device, queue_families) =
                (attributes.queue_family_picker)(physical_devices.clone())?;

            let queue_indices_set = HashSet::from([
                queue_families.graphics,
                queue_families.present,
                queue_families.transfer,
                queue_families.compute,
            ]);

            let queue_create_infos = queue_indices_set
                .iter()
                .copied()
                .map(|index| {
                    vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(index)
                        .queue_priorities(&[1.0])
                })
                .collect::<Vec<_>>();

            let device = instance.create_device(
                // logical device
                physical_device.handle,
                &vk::DeviceCreateInfo::default()
                    .queue_create_infos(&queue_create_infos)
                    .enabled_extension_names(&[ash::khr::swapchain::NAME.as_ptr()])
                    .push_next(
                        &mut vk::PhysicalDeviceDynamicRenderingFeatures::default()
                            .dynamic_rendering(true),
                    )
                    .push_next(
                        &mut vk::PhysicalDeviceBufferDeviceAddressFeatures::default()
                            .buffer_device_address(true),
                    ),
                None,
            )?;

            let swapchain_extensions = ash::khr::swapchain::Device::new(&instance, &device);

            let queues = queue_indices_set
                .iter()
                .copied()
                .map(|index| device.get_device_queue(index, 0))
                .collect::<Vec<_>>();

            Ok(Self {
                queues,
                device,
                queue_indices_set,
                queue_families,
                physical_device,
                surface_extensions,
                instance,
                entry,
                debug_messenger,
                swapchain_extensions,
            })
        }
    }

    /// # Safety
    /// The window should outlive the surface
    pub unsafe fn create_surface(&self, window: Arc<Window>) -> Result<SwapchainSurface> {
        let raw_display_handle = window.display_handle()?.as_raw();
        let raw_window_handle = window.window_handle()?.as_raw();
        unsafe {
            let handle = ash_window::create_surface(
                &self.entry,
                &self.instance,
                raw_display_handle,
                raw_window_handle,
                None,
            )?;
            let capabilities = self
                .surface_extensions
                .get_physical_device_surface_capabilities(self.physical_device.handle, handle)?;

            let formats = self
                .surface_extensions
                .get_physical_device_surface_formats(self.physical_device.handle, handle)?;

            let present_modes = self
                .surface_extensions
                .get_physical_device_surface_present_modes(self.physical_device.handle, handle)?;

            Ok(SwapchainSurface {
                handle,
                capabilities: capabilities,
                formats,
                present_modes,
            })
        }
    }

    pub fn create_swapchain(
        &self,
        create_info: &vk::SwapchainCreateInfoKHR,
    ) -> Result<vk::SwapchainKHR> {
        unsafe {
            Ok(self
                .swapchain_extensions
                .create_swapchain(&create_info, None)?)
        }
    }

    pub fn create_image_view(
        &self,
        image: vk::Image,
        format: vk::Format,
        aspect_flags: vk::ImageAspectFlags,
    ) -> Result<vk::ImageView> {
        let image_view = unsafe {
            self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .components(vk::ComponentMapping::default())
                    .subresource_range(
                        vk::ImageSubresourceRange::default()
                            .aspect_mask(aspect_flags)
                            .base_mip_level(0)
                            .level_count(1)
                            .base_array_layer(0)
                            .layer_count(1),
                    ),
                None,
            )
        }?;
        Ok(image_view)
    }
}

pub struct SwapchainSurface {
    pub handle: vk::SurfaceKHR,
    pub capabilities: vk::SurfaceCapabilitiesKHR,
    pub formats: Vec<vk::SurfaceFormatKHR>,
    pub present_modes: Vec<vk::PresentModeKHR>,
}

impl Drop for RenderingContext {
    fn drop(&mut self) {
        unsafe {
            #[cfg(debug_assertions)]
            if let Some(m) = &self.debug_messenger {
                m.destroy();
            }
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}
