//! PS5_RUFFLE_GPU_V1 — bring up a `wgpu::Device` on the statically linked RADV, in a title.
//!
//! This is the one piece with no prior art in Rust: PS5_RetroArch and PS5_Vulkan drive RADV from
//! C/C++. The console facts they established still hold and shape every step here:
//!   * A title links RADV as a static archive and resolves Vulkan through `vk_icdGetInstanceProcAddr`
//!     (PS5_RetroArch src/radv_icd_ps5.c). The shell exports it to us as `ps5_vk_get_instance_proc_addr`.
//!   * `sceAgcInit(8)` must be called from the executable before any Vulkan call; the shell does that.
//!   * The display is RADV's `VK_KHR_display` on VideoOut (Mesa patch 0011): one display, one mode,
//!     3840x2160 BGRA. We create a `VkSurfaceKHR` from it with plain Vulkan and hand it to wgpu.
//!   * A title cannot dlopen, so ash carries no loader: we build `ash::Entry` from the static symbol.
//!
//! The ash / Vulkan side below is ordinary Vulkan and should be right. The wgpu-hal adoption seam
//! (`from_raw` / `*_from_hal`) is version-sensitive (wgpu-hal 30.0.1); its exact argument lists are
//! finalized against the compiler on the server, the way milestone 1's shim was. Those spots are
//! marked `SERVER:`.
#![allow(clippy::missing_safety_doc)]

use ash::vk;
use std::ffi::{c_void, CStr};
use std::sync::Arc;

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
    pub width: u32,
    pub height: u32,
}

/// Build the ash `Entry` from the statically linked RADV, with no dynamic loader.
unsafe fn static_entry() -> ash::Entry {
    let p = ps5_vk_get_instance_proc_addr();
    assert!(!p.is_null(), "the shell returned a null vkGetInstanceProcAddr");
    let gipa: vk::PFN_vkGetInstanceProcAddr = std::mem::transmute(p);
    ash::Entry::from_static_fn(vk::StaticFn { get_instance_proc_addr: gipa })
}

/// Create the VkInstance with the extensions the display surface needs. RADV's ICD serves these.
unsafe fn create_instance(entry: &ash::Entry) -> ash::Instance {
    let app = vk::ApplicationInfo::default()
        .application_name(CStr::from_bytes_with_nul_unchecked(b"ruffle-ps5\0"))
        .api_version(vk::make_api_version(0, 1, 3, 0));
    let exts = [
        vk::KHR_SURFACE_NAME.as_ptr(),
        vk::KHR_DISPLAY_NAME.as_ptr(),
        vk::KHR_GET_PHYSICAL_DEVICE_PROPERTIES2_NAME.as_ptr(),
    ];
    let info = vk::InstanceCreateInfo::default()
        .application_info(&app)
        .enabled_extension_names(&exts);
    entry
        .create_instance(&info, None)
        .expect("vkCreateInstance on RADV failed")
}

/// Pick the console's one display and its one mode, and make a full-screen surface on plane 0.
/// Straight `VK_KHR_display`; RADV flips it through VideoOut (Mesa patch 0011).
unsafe fn create_display_surface(
    entry: &ash::Entry,
    instance: &ash::Instance,
    phys: vk::PhysicalDevice,
) -> (vk::SurfaceKHR, u32, u32) {
    let display = ash::khr::display::Instance::new(entry, instance);

    let props = display
        .get_physical_device_display_properties(phys)
        .expect("vkGetPhysicalDeviceDisplayPropertiesKHR failed");
    assert!(!props.is_empty(), "RADV reports no display");
    let disp = props[0].display;
    let panel = props[0].physical_resolution; // 3840x2160 on this console

    let modes = display
        .get_display_mode_properties(phys, disp)
        .expect("vkGetDisplayModePropertiesKHR failed");
    assert!(!modes.is_empty(), "the display has no modes");
    let mode = modes[0].display_mode;
    let extent = modes[0].parameters.visible_region;

    // Find a plane that supports this display.
    let plane_props = display
        .get_physical_device_display_plane_properties(phys)
        .expect("vkGetPhysicalDeviceDisplayPlanePropertiesKHR failed");
    let mut plane_index = 0u32;
    for (i, _) in plane_props.iter().enumerate() {
        if let Ok(supported) = display.get_display_plane_supported_displays(phys, i as u32) {
            if supported.contains(&disp) {
                plane_index = i as u32;
                break;
            }
        }
    }

    let create = vk::DisplaySurfaceCreateInfoKHR::default()
        .display_mode(mode)
        .plane_index(plane_index)
        .transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
        .alpha_mode(vk::DisplayPlaneAlphaFlagsKHR::OPAQUE)
        .image_extent(extent);
    let surface = display
        .create_display_plane_surface(&create, None)
        .expect("vkCreateDisplayPlaneSurfaceKHR failed");

    let w = if extent.width != 0 { extent.width } else { panel.width as u32 };
    let h = if extent.height != 0 { extent.height } else { panel.height as u32 };
    (surface, w, h)
}

/// Bring the whole chain up and hand back wgpu primitives + the display surface.
pub fn create() -> Result<Gpu, String> {
    unsafe {
        let entry = static_entry();
        let raw_instance = create_instance(&entry);

        // The one physical device RADV presents (PS5 GPU, NAVI21).
        let phys = raw_instance
            .enumerate_physical_devices()
            .map_err(|e| format!("enumerate_physical_devices: {e}"))?;
        let phys = *phys.first().ok_or("RADV enumerated no physical device")?;

        let (raw_surface, width, height) = create_display_surface(&entry, &raw_instance, phys);

        // ---- wgpu-hal adoption (SERVER: pin these argument lists to wgpu-hal 30.0.1) ----
        // Wrap our entry+instance as a hal Vulkan instance. The extension list we enabled must be
        // echoed here so wgpu-hal knows what is loaded.
        let instance_exts = vec![
            ash::khr::surface::NAME,
            ash::khr::display::NAME,
            ash::khr::get_physical_device_properties2::NAME,
        ];
        let hal_instance = wgpu_hal::vulkan::Instance::from_raw(
            entry,
            raw_instance.clone(),
            vk::make_api_version(0, 1, 3, 0), // instance_api_version
            0,                                // android_sdk_version
            None,                             // debug_utils_create_info
            instance_exts,
            wgpu_hal::InstanceFlags::empty(),
            Default::default(),               // memory_budget_thresholds / memory hints
            false,                            // has_nv_optimus
            None,                             // drop_guard — we keep raw_instance alive via wgpu
        )
        .map_err(|e| format!("wgpu_hal vulkan Instance::from_raw: {e:?}"))?;

        // The hal surface from our raw VkSurfaceKHR.
        let hal_surface = hal_instance.create_surface_from_vk_surface_khr(raw_surface);

        // The hal adapter from our physical device.
        let hal_adapter = hal_instance
            .expose_adapter(phys)
            .ok_or("wgpu-hal could not expose the RADV physical device")?;

        // ---- into wgpu ----
        let instance = wgpu::Instance::from_hal::<wgpu_hal::vulkan::Api>(hal_instance);
        let surface = instance
            .create_surface_from_hal::<wgpu_hal::vulkan::Api>(hal_surface)
            .map_err(|e| format!("create_surface_from_hal: {e:?}"))?;
        let adapter = instance.create_adapter_from_hal::<wgpu_hal::vulkan::Api>(hal_adapter);

        let (device, queue) = {
            // Open the device from the hal adapter so RADV's features/limits carry over.
            let open = hal_adapter_open(&adapter)?;
            adapter
                .create_device_from_hal(
                    open,
                    &wgpu::DeviceDescriptor {
                        label: Some("ruffle-ps5 RADV"),
                        required_features: wgpu::Features::empty(),
                        required_limits: adapter.limits(),
                        memory_hints: wgpu::MemoryHints::Performance,
                        trace: wgpu::Trace::Off,
                    },
                    None,
                )
                .map_err(|e| format!("create_device_from_hal: {e:?}"))?
        };

        // Configure the surface to the display size, mailbox/fifo as RADV allows.
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb))
            .unwrap_or(caps.formats[0]);
        surface.configure(
            &device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );

        Ok(Gpu { instance, adapter, device, queue, surface, width, height })
    }
}

/// SERVER: wgpu-hal 30's `Adapter::open` returns `OpenDevice`; wire the exact call once the crate is
/// resolved. Kept separate so the signature churn is in one place.
fn hal_adapter_open(
    _adapter: &wgpu::Adapter,
) -> Result<wgpu_hal::OpenDevice<wgpu_hal::vulkan::Api>, String> {
    // The hal adapter is inside `adapter`; wgpu 30 exposes it through `adapter.as_hal::<Api,_,_>()`.
    // Opening yields queues + device. Finalized on the server where the method shapes are visible.
    Err("hal_adapter_open: finalize against wgpu-hal 30.0.1 on the server".to_string())
}

/// Convenience used by milestone 3a (clear the screen) and 3b (Ruffle). Wraps the surface frame.
pub fn present_clear(gpu: &Gpu, r: f64, g: f64, b: f64) -> Result<(), String> {
    let frame = gpu
        .surface
        .get_current_texture()
        .map_err(|e| format!("get_current_texture: {e:?}"))?;
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("clear") });
    {
        let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    }
    gpu.queue.submit(Some(enc.finish()));
    frame.present();
    Ok(())
}

/// Shared descriptors for Ruffle (milestone 3b): wrap the wgpu primitives Ruffle's renderer wants.
pub fn descriptors(gpu: &Gpu) -> Arc<ruffle_render_wgpu::descriptors::Descriptors> {
    Arc::new(ruffle_render_wgpu::descriptors::Descriptors::new(
        gpu.instance.clone(),
        gpu.adapter.clone(),
        gpu.device.clone(),
        gpu.queue.clone(),
    ))
}
