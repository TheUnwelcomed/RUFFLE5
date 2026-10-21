//! PS5_RUFFLE_GPU_V2 — bring up a `wgpu::Device` on the statically linked RADV, in a title.
//!
//! This is the one piece with no prior art in Rust: PS5_RetroArch and PS5_Vulkan drive RADV from
//! C/C++. The console facts they established shape every step here:
//!   * A title links RADV as a static archive and resolves Vulkan through `vk_icdGetInstanceProcAddr`
//!     (PS5_RetroArch src/radv_icd_ps5.c). The shell hands it to us as `ps5_vk_get_instance_proc_addr`.
//!   * The display is RADV's `VK_KHR_display` on VideoOut (the Mesa fork's wsi_common_videoout.c):
//!     one display, one plane, a mode per size VideoOut takes (3840x2160 first, 2560x1440,
//!     1920x1080), FIFO the only present mode, BGRA8 unorm/srgb the formats.
//!   * A title cannot dlopen, so ash carries no loader: `ash::Entry` is built from the static symbol.
//!
//! Every call below is pinned to wgpu 30.0.1 / wgpu-hal 30.0.1 / ash 0.38 (read from their sources):
//!   `wgpu_hal::vulkan::Instance::from_raw`, `expose_adapter`, `wgpu_hal::Adapter::open`,
//!   `wgpu::Instance::from_hal`, `as_hal`, `create_surface_unsafe`, `create_adapter_from_hal`,
//!   `Adapter::create_device_from_hal`.
//! The surface is the one step wgpu 30 has no door for: a `VkSurfaceKHR` made by the application
//! cannot be adopted (its only display-less route wants a Linux DRM fd). build-title.sh applies
//! tools/wgpu-hal-ps5-surface.patch, which teaches wgpu-hal's Vulkan backend to take the surface
//! handle through a raw-window-handle (`Gbm`, the one variant that is a single opaque pointer).
#![allow(clippy::missing_safety_doc)]

use ash::vk;
use ash::vk::Handle as _;
use raw_window_handle::{GbmDisplayHandle, GbmWindowHandle, RawDisplayHandle, RawWindowHandle};
use std::ffi::{c_char, c_void, CStr};
use std::ptr::NonNull;
use wgpu_hal::Adapter as _;

extern "C" {
    /// The RADV archive's ICD entry point, exported by the C shell. Same signature and semantics as
    /// `vkGetInstanceProcAddr` (it answers the global commands for a NULL instance), which is why
    /// PS5_RetroArch resolves the whole API through it.
    fn ps5_vk_get_instance_proc_addr() -> *const c_void;
}

/// Everything the renderer must keep alive for the life of the title (dropped in reverse order).
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface<'static>,
    pub format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
    /// What was found and chosen, for the log.
    pub report: String,
}

/// The display size to ask for. VideoOut scales any of its sizes to the screen; 1080p is plenty for
/// Flash content and a quarter of the fill of 2160p. The first mode (3840x2160) is the fallback.
const PREFERRED_MODE: (u32, u32) = (1920, 1080);

/// Build the ash `Entry` from the statically linked RADV, with no dynamic loader.
unsafe fn static_entry() -> Result<ash::Entry, String> {
    let p = ps5_vk_get_instance_proc_addr();
    if p.is_null() {
        return Err("the shell returned a null vkGetInstanceProcAddr".into());
    }
    let gipa: vk::PFN_vkGetInstanceProcAddr = std::mem::transmute(p);
    Ok(ash::Entry::from_static_fn(ash::StaticFn {
        get_instance_proc_addr: gipa,
    }))
}

/// The instance extensions wgpu-hal would ask for on this driver (it filters by what is
/// available), plus VK_KHR_display for the surface. Returned as the `'static` names wgpu-hal keeps.
unsafe fn instance_extensions(
    entry: &ash::Entry,
    api_version: u32,
) -> Result<Vec<&'static CStr>, String> {
    let mut exts = wgpu_hal::vulkan::Instance::desired_extensions(
        entry,
        api_version,
        wgpu::InstanceFlags::empty(),
    )
    .map_err(|e| format!("desired_extensions: {e:?}"))?;
    let available = entry
        .enumerate_instance_extension_properties(None)
        .map_err(|e| format!("vkEnumerateInstanceExtensionProperties: {e}"))?;
    let has = |name: &CStr| {
        available
            .iter()
            .any(|p| p.extension_name_as_c_str() == Ok(name))
    };
    for wanted in [ash::khr::surface::NAME, ash::khr::display::NAME] {
        if !exts.contains(&wanted) {
            if !has(wanted) {
                return Err(format!("RADV does not offer {}", wanted.to_string_lossy()));
            }
            exts.push(wanted);
        }
    }
    Ok(exts)
}

/// Pick the console's one display and a mode, and make a full-screen surface on a plane that
/// supports it. Straight `VK_KHR_display`; RADV flips it through VideoOut.
unsafe fn create_display_surface(
    entry: &ash::Entry,
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
    report: &mut String,
) -> Result<(vk::SurfaceKHR, vk::Extent2D), String> {
    let display = ash::khr::display::Instance::new(entry, instance);

    let props = display
        .get_physical_device_display_properties(phys)
        .map_err(|e| format!("vkGetPhysicalDeviceDisplayPropertiesKHR: {e}"))?;
    let first = props.first().ok_or("RADV reports no display")?;
    let disp = first.display;
    report.push_str(&format!(
        "display: {} {}x{} ",
        first
            .display_name_as_c_str()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        first.physical_resolution.width,
        first.physical_resolution.height
    ));

    let modes = display
        .get_display_mode_properties(phys, disp)
        .map_err(|e| format!("vkGetDisplayModePropertiesKHR: {e}"))?;
    if modes.is_empty() {
        return Err("the display has no modes".into());
    }
    for m in &modes {
        report.push_str(&format!(
            "[{}x{}@{}] ",
            m.parameters.visible_region.width,
            m.parameters.visible_region.height,
            m.parameters.refresh_rate
        ));
    }
    let chosen = modes
        .iter()
        .find(|m| {
            m.parameters.visible_region.width == PREFERRED_MODE.0
                && m.parameters.visible_region.height == PREFERRED_MODE.1
        })
        .unwrap_or(&modes[0]);
    let extent = chosen.parameters.visible_region;
    report.push_str(&format!("-> {}x{}; ", extent.width, extent.height));

    // A plane that supports this display (the console has one).
    let plane_props = display
        .get_physical_device_display_plane_properties(phys)
        .map_err(|e| format!("vkGetPhysicalDeviceDisplayPlanePropertiesKHR: {e}"))?;
    let mut plane_index = 0u32;
    for i in 0..plane_props.len() as u32 {
        if let Ok(supported) = display.get_display_plane_supported_displays(phys, i) {
            if supported.contains(&disp) {
                plane_index = i;
                break;
            }
        }
    }

    let create = vk::DisplaySurfaceCreateInfoKHR::default()
        .display_mode(chosen.display_mode)
        .plane_index(plane_index)
        .plane_stack_index(0)
        .transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
        .global_alpha(1.0)
        .alpha_mode(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE)
        .image_extent(extent);
    let surface = display
        .create_display_plane_surface(&create, None)
        .map_err(|e| format!("vkCreateDisplayPlaneSurfaceKHR: {e}"))?;
    Ok((surface, extent))
}

/// Bring the whole chain up and hand back the wgpu primitives + the configured display surface.
pub fn create() -> Result<Gpu, String> {
    unsafe {
        let mut report = String::new();
        let entry = static_entry()?;

        // The instance version RADV reports (1.1+ has vkEnumerateInstanceVersion), as wgpu-hal's own
        // init does; the app_info api_version below is that too.
        let api_version = match entry.try_enumerate_instance_version() {
            Ok(Some(v)) => v,
            Ok(None) => vk::API_VERSION_1_0,
            Err(e) => return Err(format!("vkEnumerateInstanceVersion: {e}")),
        };
        report.push_str(&format!(
            "instance {}.{}.{}; ",
            vk::api_version_major(api_version),
            vk::api_version_minor(api_version),
            vk::api_version_patch(api_version)
        ));

        let exts = instance_extensions(&entry, api_version)?;
        let ext_ptrs: Vec<*const c_char> = exts.iter().map(|e| e.as_ptr()).collect();
        let app = vk::ApplicationInfo::default()
            .application_name(c"ruffle-ps5")
            .application_version(1)
            .engine_name(c"wgpu-hal")
            .api_version(api_version);
        let info = vk::InstanceCreateInfo::default()
            .application_info(&app)
            .enabled_extension_names(&ext_ptrs);
        let raw_instance = entry
            .create_instance(&info, None)
            .map_err(|e| format!("vkCreateInstance on RADV: {e}"))?;

        // The one physical device RADV presents (PlayStation 5 GPU, NAVI21).
        let phys_all = raw_instance
            .enumerate_physical_devices()
            .map_err(|e| format!("vkEnumeratePhysicalDevices: {e}"))?;
        let phys = *phys_all
            .first()
            .ok_or("RADV enumerated no physical device")?;
        {
            let p = raw_instance.get_physical_device_properties(phys);
            report.push_str(&format!(
                "device {} (api {}.{}.{}); ",
                p.device_name_as_c_str()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                vk::api_version_major(p.api_version),
                vk::api_version_minor(p.api_version),
                vk::api_version_patch(p.api_version)
            ));
        }

        let (raw_surface, extent) =
            create_display_surface(&entry, &raw_instance, phys, &mut report)?;
        let (width, height) = (extent.width, extent.height);

        // ---- wgpu-hal adopts the instance (and owns it from here: it destroys it on drop) ----
        let hal_instance = wgpu_hal::vulkan::Instance::from_raw(
            entry.clone(),
            raw_instance.clone(),
            api_version,
            0,    // android_sdk_version
            None, // debug utils: none (no validation layer in a title)
            exts,
            wgpu::InstanceFlags::empty(),
            wgpu::MemoryBudgetThresholds::default(),
            false, // has_nv_optimus
            None,  // drop_callback: wgpu-hal takes ownership of raw_instance
        )
        .map_err(|e| format!("wgpu_hal::vulkan::Instance::from_raw: {e:?}"))?;

        // ---- into wgpu ----
        let instance = wgpu::Instance::from_hal::<wgpu_hal::vulkan::Api>(hal_instance);

        // The surface: our VkSurfaceKHR, through the patched wgpu-hal (tools/wgpu-hal-ps5-surface.patch).
        // wgpu owns it from here and destroys it with the wgpu::Surface.
        let surface_ptr = NonNull::new(raw_surface.as_raw() as usize as *mut c_void)
            .ok_or("vkCreateDisplayPlaneSurfaceKHR returned a null handle")?;
        let surface = instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(RawDisplayHandle::Gbm(GbmDisplayHandle::new(
                    NonNull::dangling(),
                ))),
                raw_window_handle: RawWindowHandle::Gbm(GbmWindowHandle::new(surface_ptr)),
            })
            .map_err(|e| {
                format!("create_surface_unsafe (is tools/wgpu-hal-ps5-surface.patch applied?): {e}")
            })?;

        // The adapter from our physical device, and the device opened from the hal adapter so
        // RADV's features/limits carry over. Features and limits as Ruffle's own request_device asks
        // for them (render/wgpu/src/backend.rs): GL-ES-3-class limits raised to the adapter's
        // resolution and alignment, the optional features the adapter has.
        let hal_adapter = instance
            .as_hal::<wgpu_hal::vulkan::Api>()
            .ok_or("wgpu lost its Vulkan instance")?
            .expose_adapter(phys)
            .ok_or("wgpu-hal would not expose the RADV physical device as an adapter")?;
        report.push_str(&format!(
            "adapter {} ({:?}, driver {} {}); ",
            hal_adapter.info.name,
            hal_adapter.info.device_type,
            hal_adapter.info.driver,
            hal_adapter.info.driver_info
        ));
        let adapter_limits = hal_adapter.capabilities.limits.clone();
        let mut limits = wgpu::Limits::downlevel_webgl2_defaults()
            .using_resolution(adapter_limits.clone())
            .using_alignment(adapter_limits.clone());
        limits.max_uniform_buffer_binding_size = adapter_limits.max_uniform_buffer_binding_size;
        limits.max_inter_stage_shader_variables = adapter_limits.max_inter_stage_shader_variables;
        limits.max_color_attachments = 4;
        let optional_features = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::TEXTURE_COMPRESSION_BC
            | wgpu::Features::FLOAT32_FILTERABLE
            | wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
        let features = optional_features & hal_adapter.features;
        let memory_hints = wgpu::MemoryHints::Performance;
        let open = hal_adapter
            .adapter
            .open(features, &limits, &memory_hints)
            .map_err(|e| format!("wgpu-hal open (vkCreateDevice on RADV): {e:?}"))?;
        let adapter = instance.create_adapter_from_hal::<wgpu_hal::vulkan::Api>(hal_adapter);
        let (device, queue) = adapter
            .create_device_from_hal::<wgpu_hal::vulkan::Api>(
                open,
                &wgpu::DeviceDescriptor {
                    label: Some("ruffle-ps5 RADV"),
                    required_features: features,
                    required_limits: limits,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints,
                    trace: wgpu::Trace::Off,
                },
            )
            .map_err(|e| format!("create_device_from_hal: {e}"))?;

        // The surface at the display size. VideoOut's WSI offers FIFO only and BGRA8; Flash colours
        // are sRGB values written as they are, so the unorm view is what Ruffle wants too.
        let caps = surface.get_capabilities(&adapter);
        report.push_str(&format!(
            "surface formats {:?}, present modes {:?}, alpha {:?}",
            caps.formats, caps.present_modes, caps.alpha_modes
        ));
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
            .or_else(|| {
                caps.formats
                    .iter()
                    .copied()
                    .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb)
            })
            .or_else(|| caps.formats.first().copied())
            .ok_or("the surface reports no formats")?;
        let alpha_mode = if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto)
        };
        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                color_space: wgpu::SurfaceColorSpace::Auto,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );

        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
            surface,
            format,
            width,
            height,
            report,
        })
    }
}

/// Milestone 3a: clear the display to a colour and present it. One frame.
pub fn present_clear(gpu: &Gpu, r: f64, g: f64, b: f64) -> Result<(), String> {
    // wgpu 30: an enum, not a Result; presenting goes through the queue.
    let frame = match gpu.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
        other => return Err(format!("get_current_texture: {other:?}")),
    };
    let view = frame
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("clear"),
        });
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
    gpu.queue.submit(Some(enc.finish()));
    gpu.queue.present(frame);
    Ok(())
}

/// Shared descriptors for Ruffle (milestone 3b): wrap the wgpu primitives Ruffle's renderer wants.
#[cfg(feature = "ruffle")]
pub fn descriptors(gpu: &Gpu) -> std::sync::Arc<ruffle_render_wgpu::descriptors::Descriptors> {
    std::sync::Arc::new(ruffle_render_wgpu::descriptors::Descriptors::new(
        gpu.instance.clone(),
        gpu.adapter.clone(),
        gpu.device.clone(),
        gpu.queue.clone(),
    ))
}
