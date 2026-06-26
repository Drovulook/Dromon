use ash::vk;

#[derive(Clone, Copy, Debug)]
pub struct ImageLayoutState {
    pub access_mask: vk::AccessFlags2,
    pub layout: vk::ImageLayout,
    pub stage_mask: vk::PipelineStageFlags2,
    pub queue_family_index: u32,
}

impl Default for ImageLayoutState {
    fn default() -> Self {
        Self {
            access_mask: vk::AccessFlags2::NONE,
            layout: vk::ImageLayout::UNDEFINED,
            stage_mask: vk::PipelineStageFlags2::ALL_COMMANDS,
            queue_family_index: vk::QUEUE_FAMILY_IGNORED,
        }
    }
}

impl ImageLayoutState {
    // color image
    pub const UNDEFINED_COLOR_IMAGE_STATE: Self = Self {
        access_mask: vk::AccessFlags2::empty(),
        layout: vk::ImageLayout::UNDEFINED,
        stage_mask: vk::PipelineStageFlags2::TOP_OF_PIPE,
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };
    pub const RENDERABLE_COLOR_IMAGE_STATE: Self = Self {
        access_mask: vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };
    pub const PRESENT_COLOR_IMAGE_STATE: Self = Self {
        access_mask: vk::AccessFlags2::empty(),
        layout: vk::ImageLayout::PRESENT_SRC_KHR,
        stage_mask: vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };

    // depth image
    pub const UNDEFINED_DEPTH_IMAGE_STATE: Self = Self {
        access_mask: vk::AccessFlags2::empty(),
        layout: vk::ImageLayout::UNDEFINED,
        stage_mask: vk::PipelineStageFlags2::TOP_OF_PIPE,
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };
    pub const RENDERABLE_DEPTH_IMAGE_STATE: Self = Self {
        access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
        layout: vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        stage_mask: vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS,
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };

    // shadow map : cible de rendu de la passe d'ombre (on y écrit la profondeur).
    // On couvre EARLY+LATE fragment tests : l'écriture de profondeur se termine
    // en LATE, et c'est ce qu'on doit attendre avant de relire la carte.
    pub const SHADOW_MAP_WRITE_STATE: Self = Self {
        access_mask: vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
        layout: vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        stage_mask: vk::PipelineStageFlags2::from_raw(
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS.as_raw()
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS.as_raw(),
        ),
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };
    // shadow map : lecture (échantillonnage) dans le frag shader de la passe principale.
    pub const SHADOW_MAP_READ_STATE: Self = Self {
        access_mask: vk::AccessFlags2::SHADER_READ,
        layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
        queue_family_index: vk::QUEUE_FAMILY_IGNORED,
    };
}
