use anyhow::Result;
use ash::vk;
use std::{collections::HashSet, sync::Arc};
use winit::{
    raw_window_handle::{HasDisplayHandle, HasWindowHandle},
    window::Window,
};

pub struct Context {
    pub queues: Vec<vk::Queue>,
    pub device: ash::Device,
    pub queue_indices_set: HashSet<u32>,
    pub queue_families: QueueFamilies,
    pub physical_devices: Vec<PhysicalDevice>,
    pub surface: ash::vk::SurfaceKHR,
    pub surface_extensions: ash::khr::surface::Instance,
    pub instance: ash::Instance,
    pub entry: ash::Entry,
    pub attributes: ContextAttributes,
}

type QueueFamilyPicker = fn(Vec<PhysicalDevice>) -> Result<(PhysicalDevice, QueueFamilies)>;

pub struct ContextAttributes {
    pub window: Arc<Window>,
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
    use crate::app::engine::renderer::context::{PhysicalDevice, QueueFamilies};
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

impl Context {
    pub fn new(attributes: ContextAttributes) -> Result<Self> {
        unsafe {
            // TODO: créer entry et instance une seule fois, pas une fois / renderer
            let entry = ash::Entry::load()?;
            let raw_display_handle = attributes.window.display_handle()?.as_raw();
            let raw_window_handle = attributes.window.window_handle()?.as_raw();

            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default()
                    .application_info(
                        &vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_3),
                    )
                    .enabled_extension_names(ash_window::enumerate_required_extensions(
                        raw_display_handle,
                    )?),
                None,
            )?;

            let surface_extensions = ash::khr::surface::Instance::new(&entry, &instance);
            let surface = ash_window::create_surface(
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

                    // let name = std::ffi::CStr::from_ptr(properties.device_name.as_ptr());
                    // dbg!(name, props.device_type, props.api_version);

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
                    .get_physical_device_surface_support(device.handle, 0, surface)
                    .unwrap_or(false)
            });

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
                physical_devices,
                surface,
                surface_extensions,
                instance,
                entry,
                attributes,
            })
        }
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            self.surface_extensions.destroy_surface(self.surface, None);
        }
    }
}
