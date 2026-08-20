//! The wgpu backend: Metal, Vulkan and DX12 behind one implementation of
//! [`Backend`].
//!
//! One artifact per platform, so this is always compiled in and asks the
//! machine at run time what it has. A machine with no adapter — a CI runner,
//! a container — finds none, [`shared`] answers None, and every program
//! keeps running on the CPU.
//!
//! Shaders arrive as WGSL text and are compiled by the driver on first use;
//! the compiled pipelines are cached per source and entry point, so a kernel
//! run repeatedly pays for its compilation once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use super::{Backend, DeviceError, DeviceInfo, Handle, Plan};

/// The instance, built once. Creating one enumerates the platform's
/// drivers, which is not cheap and not worth repeating.
fn instance() -> Option<&'static wgpu::Instance> {
    static INSTANCE: OnceLock<Option<wgpu::Instance>> = OnceLock::new();
    INSTANCE
        .get_or_init(|| {
            // No backend compiled for this platform: `Instance::new` would
            // panic rather than report it.
            if wgpu::Instance::enabled_backend_features().is_empty() {
                return None;
            }
            // `from_env_or_default` honours wgpu's own `WGPU_BACKEND` and
            // friends, which is how a host — or a test that wants the
            // no-adapter path — restricts what is looked for.
            Some(wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default()))
        })
        .as_ref()
}

fn kind_name(t: wgpu::DeviceType) -> &'static str {
    match t {
        wgpu::DeviceType::DiscreteGpu => "discrete GPU",
        wgpu::DeviceType::IntegratedGpu => "integrated GPU",
        wgpu::DeviceType::VirtualGpu => "virtual GPU",
        wgpu::DeviceType::Cpu => "CPU",
        wgpu::DeviceType::Other => "other",
    }
}

fn describe(a: &wgpu::Adapter) -> DeviceInfo {
    let info = a.get_info();
    DeviceInfo {
        name: info.name,
        backend: info.backend.to_string(),
        kind: kind_name(info.device_type).to_string(),
        f64: a.features().contains(wgpu::Features::SHADER_F64),
    }
}

/// Every adapter the platform reports.
pub(super) fn enumerate() -> Vec<DeviceInfo> {
    let Some(inst) = instance() else { return Vec::new() };
    inst.enumerate_adapters(wgpu::Backends::all()).iter().map(describe).collect()
}

/// The machine's preferred adapter, opened once and shared.
pub(super) fn shared() -> Option<Arc<dyn Backend>> {
    static GPU: OnceLock<Option<Arc<Gpu>>> = OnceLock::new();
    let gpu = GPU.get_or_init(open).clone()?;
    Some(gpu)
}

fn open() -> Option<Arc<Gpu>> {
    let inst = instance()?;
    let adapter = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok()?;
    let info = describe(&adapter);
    // f64 where the adapter has it; the placement rules above decide what
    // to do when it has not.
    let mut features = wgpu::Features::empty();
    if info.f64 {
        features |= wgpu::Features::SHADER_F64;
    }
    // The adapter's own limits rather than the portable defaults: those cap
    // a storage buffer at 128 MiB, which a 20M-row f64 column exceeds.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("libjay"),
        required_features: features,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    // An error that escapes an error scope would otherwise reach wgpu's
    // default handler, which panics. Every failure here is a fallback.
    device.on_uncaptured_error(Box::new(|e| {
        eprintln!("libjay: the device reported an error: {e}");
    }));
    Some(Arc::new(Gpu {
        info,
        device,
        queue,
        shaders: Mutex::new(HashMap::new()),
        pipelines: Mutex::new(HashMap::new()),
    }))
}

struct Gpu {
    info: DeviceInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Compiled modules, by the source they were compiled from.
    shaders: Mutex<HashMap<String, wgpu::ShaderModule>>,
    /// Compiled pipelines, by source and entry point.
    pipelines: Mutex<HashMap<(String, String), wgpu::ComputePipeline>>,
}

impl Gpu {
    /// Run `f` with validation errors collected rather than raised.
    fn scoped<T>(&self, f: impl FnOnce() -> T) -> Result<T, DeviceError> {
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let v = f();
        match pollster::block_on(self.device.pop_error_scope()) {
            None => Ok(v),
            Some(e) => Err(DeviceError(e.to_string())),
        }
    }

    fn pipeline(&self, source: &str, entry: &str) -> Result<wgpu::ComputePipeline, DeviceError> {
        let key = (source.to_string(), entry.to_string());
        if let Some(p) = self.pipelines.lock().expect("pipeline cache").get(&key) {
            return Ok(p.clone());
        }
        let module = {
            let mut shaders = self.shaders.lock().expect("shader cache");
            match shaders.get(source) {
                Some(m) => m.clone(),
                None => {
                    let m = self.scoped(|| {
                        self.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                            label: Some("libjay kernel"),
                            source: wgpu::ShaderSource::Wgsl(source.into()),
                        })
                    })?;
                    shaders.insert(source.to_string(), m.clone());
                    m
                }
            }
        };
        let pipeline = self.scoped(|| {
            self.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("libjay kernel"),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        })?;
        self.pipelines.lock().expect("pipeline cache").insert(key, pipeline.clone());
        Ok(pipeline)
    }
}

/// Mapping a buffer wants both ends aligned; rounding a buffer up to that
/// costs at most seven bytes.
const MAP_ALIGN: usize = 8;

impl Backend for Gpu {
    fn info(&self) -> &DeviceInfo {
        &self.info
    }

    fn upload(&self, values: &[f64], p: super::Precision) -> Result<Handle, DeviceError> {
        let bytes = values.len() * p.size();
        let size = bytes.max(MAP_ALIGN).next_multiple_of(MAP_ALIGN) as u64;
        // Mapped at creation, so the elements are converted straight into
        // the buffer the device will read: an argument here is tens of
        // megabytes and every extra pass over it is visible in the timing.
        let buffer = self.scoped(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("libjay input"),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: true,
            })
        })?;
        {
            let mut view = buffer.get_mapped_range_mut(..);
            super::codegen::write_bytes(&mut view[..bytes], values, p);
        }
        buffer.unmap();
        Ok(Handle(Arc::new(buffer)))
    }

    fn dispatch(&self, plan: &Plan<'_>) -> Result<Vec<u8>, DeviceError> {
        let pipeline = self.pipeline(plan.source, plan.entry)?;
        let bytes = (plan.out_elems * plan.elem_size).next_multiple_of(MAP_ALIGN) as u64;
        self.scoped(|| self.run(plan, &pipeline, bytes))?
            .map(|mut v| {
                v.truncate(plan.out_elems * plan.elem_size);
                v
            })
    }
}

impl Gpu {
    fn run(
        &self,
        plan: &Plan<'_>,
        pipeline: &wgpu::ComputePipeline,
        bytes: u64,
    ) -> Result<Vec<u8>, DeviceError> {
        // Padded to 16 so that the struct satisfies the uniform address
        // space's layout rules on every backend.
        let meta = [plan.n, plan.stride, 0u32, 0u32];
        let meta_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libjay meta"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&meta_buf, 0, bytemuck_u32(&meta));

        let out_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libjay out"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("libjay readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: meta_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: out_buf.as_entire_binding() },
        ];
        let inputs: Vec<&wgpu::Buffer> = plan
            .inputs
            .iter()
            .map(|h| {
                h.0.downcast_ref::<wgpu::Buffer>()
                    .expect("a handle this backend made")
            })
            .collect();
        for (i, b) in inputs.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: i as u32 + 2,
                resource: b.as_entire_binding(),
            });
        }
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("libjay bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("libjay") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("libjay kernel"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(plan.groups, 1, 1);
        }
        enc.copy_buffer_to_buffer(&out_buf, 0, &staging, 0, bytes);
        self.queue.submit(Some(enc.finish()));

        let done = Arc::new(Mutex::new(None));
        let flag = done.clone();
        staging.map_async(wgpu::MapMode::Read, .., move |r| {
            *flag.lock().expect("map result") = Some(r);
        });
        self.device
            .poll(wgpu::PollType::Wait)
            .map_err(|e| DeviceError(format!("waiting for the device: {e}")))?;
        match done.lock().expect("map result").take() {
            Some(Ok(())) => {}
            Some(Err(e)) => return Err(DeviceError(format!("reading back: {e}"))),
            None => return Err(DeviceError("the device never finished".into())),
        }
        let out = staging.get_mapped_range(..).to_vec();
        staging.unmap();
        Ok(out)
    }
}

/// Four `u32` as the bytes a uniform buffer takes.
fn bytemuck_u32(v: &[u32; 4]) -> &[u8] {
    // SAFETY: `u32` has no padding and no invalid bit patterns, and the
    // slice is read as bytes for the length of the same allocation.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}
