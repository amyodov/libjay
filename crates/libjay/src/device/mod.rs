//! Where a compiled expression runs.
//!
//! Placement is deliberately not part of binding. A kernel bound to data is
//! the same kernel wherever it executes; a [`Device`] says which processor
//! executes it, and the CPU is one of the answers. `Program::run_on` takes
//! the device explicitly, and everything a device cannot do falls back to
//! the CPU path with a reason a caller can read.
//!
//! What runs on a GPU this phase is the fused elementwise kernel and nothing
//! else. [`crate::fuse`] already compiles a chain of scalar verbs into a
//! postfix program over blocks, with an optional reduction folded in — that
//! is a kernel description, and `codegen` turns it into WGSL at run time.
//! Anything outside a fused node, and any fused node the generator declines,
//! runs where it always ran.
//!
//! # Precision
//!
//! libjay computes floats in f64. WGSL can express f64, but almost no
//! adapter implements it: Metal has no double at all, and on Vulkan it is a
//! feature (`SHADER_F64`) that many drivers leave off. A device that cannot
//! run f64 therefore **declines** by default rather than quietly computing
//! in f32 — losing precision is not a performance decision libjay may take
//! on the caller's behalf. `Precision::F32` is the caller saying, in so many
//! words, that they want it.
//!
//! # Residency
//!
//! [`Device::upload`] returns an array that carries its own location: the
//! buffer it hands back keeps the device allocation alive inside its owner
//! handle, so passing it to a later run uploads nothing. The array is an
//! ordinary [`Array`] otherwise, which is what lets a fallback to the CPU
//! read it without asking anyone.

mod codegen;
mod gpu;

use std::any::Any;
use std::sync::Arc;

use crate::array::{Array, Buf, Data, Owner};
use crate::dtype::DType;
use crate::fuse::{FusedKernel, Yield};

pub use codegen::Precision;

/// One adapter, as the machine reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The adapter's own name, e.g. "AMD Radeon Pro 560".
    pub name: String,
    /// The API behind it: Metal, Vulkan, DX12.
    pub backend: String,
    /// discrete GPU, integrated GPU, virtual GPU, CPU, or other.
    pub kind: String,
    /// Whether shaders on this adapter can compute in f64. Where this is
    /// false, only an explicit `Precision::F32` reaches the device.
    pub f64: bool,
}

/// Every adapter this machine offers, in the order the backend ranks them.
/// Empty on a machine with no GPU, which is not an error.
pub fn available() -> Vec<DeviceInfo> {
    gpu::enumerate()
}

/// Where a program runs.
///
/// Cloning is cheap: the GPU handle is shared, so two clones name the same
/// adapter and the same uploaded buffers.
#[derive(Clone)]
pub struct Device {
    at: Where,
    precision: Precision,
}

#[derive(Clone)]
enum Where {
    Cpu,
    Gpu(Arc<dyn Backend>),
}

impl std::fmt::Debug for Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.at {
            Where::Cpu => write!(f, "Device(cpu)"),
            Where::Gpu(g) => {
                write!(f, "Device({}, {:?})", g.info().name, self.precision)
            }
        }
    }
}

impl Device {
    /// The processor everything already ran on.
    pub fn cpu() -> Device {
        Device { at: Where::Cpu, precision: Precision::F64 }
    }

    /// The machine's preferred adapter, or None where there is none.
    ///
    /// The adapter is opened once per process and shared; asking twice
    /// costs nothing and hands back the same device.
    pub fn default_gpu() -> Option<Device> {
        Some(Device { at: Where::Gpu(gpu::shared()?), precision: Precision::F64 })
    }

    /// The same device, computing in `p`.
    ///
    /// `Precision::F32` is an explicit request to compute a f64 program in
    /// single precision. It is the only way a machine whose shaders have no
    /// f64 runs anything at all on its GPU.
    pub fn with_precision(&self, p: Precision) -> Device {
        Device { at: self.at.clone(), precision: p }
    }

    pub fn precision(&self) -> Precision {
        self.precision
    }

    pub fn is_gpu(&self) -> bool {
        matches!(self.at, Where::Gpu(_))
    }

    /// What this device is, or None for the CPU.
    pub fn info(&self) -> Option<&DeviceInfo> {
        match &self.at {
            Where::Cpu => None,
            Where::Gpu(g) => Some(g.info()),
        }
    }

    fn backend(&self) -> Option<&Arc<dyn Backend>> {
        match &self.at {
            Where::Cpu => None,
            Where::Gpu(g) => Some(g),
        }
    }

    /// `y` with its elements resident on this device.
    ///
    /// The result is an ordinary array — same shape, same values, readable
    /// by anything — that additionally holds the device allocation, so a
    /// run that reaches the device with it uploads nothing. Uploading to
    /// the CPU is the identity.
    pub fn upload(&self, y: &Array) -> Result<Array, DeviceError> {
        let Some(backend) = self.backend() else { return Ok(y.clone()) };
        // What goes to the device is the elements in row-major order; a
        // column-major argument is laid out once before it leaves.
        let laid_out;
        let y = if y.is_row_major() {
            y
        } else {
            laid_out = y.to_row_major();
            &laid_out
        };
        // A float array is uploaded from its own buffer; anything else is
        // converted once, and the conversion becomes the host mirror.
        let host = match &y.data {
            Data::F64(_) => Host::Same(y.data.clone()),
            Data::I64(v) => Host::Made(v.iter().map(|&x| x as f64).collect()),
            Data::Bool(v) => Host::Made(v.iter().map(|&x| x as f64).collect()),
            _ => {
                return Err(DeviceError(
                    "only boolean, integer and float arrays can be uploaded".into(),
                ))
            }
        };
        let handle = backend.upload(host.values(), self.precision)?;
        let resident = Arc::new(Resident {
            device: Arc::as_ptr(backend) as *const () as usize,
            precision: self.precision,
            elems: host.values().len(),
            handle,
            host,
        });
        let values = resident.host.values();
        let (ptr, len) = (values.as_ptr(), values.len());
        // SAFETY: the elements live inside the `Arc` this owner holds — in
        // the array's own refcounted buffer or in the vector made for the
        // upload — so they stay valid and unmutated for as long as the
        // buffer that borrows them does.
        let owner: Owner = resident;
        Ok(Array::new(y.shape.clone(), Data::F64(unsafe { Buf::foreign(ptr, len, owner) })))
    }

    /// Is this array already resident on this device, at this precision?
    pub fn holds(&self, y: &Array) -> bool {
        self.backend().is_some_and(|b| resident_on(y, b, self.precision).is_some())
    }
}

/// The elements an uploaded array's buffer borrows: the array's own, when
/// it was already f64, or the conversion the upload had to make anyway.
enum Host {
    Same(Data),
    Made(Vec<f64>),
}

impl Host {
    fn values(&self) -> &[f64] {
        match self {
            Host::Same(Data::F64(v)) => v.as_slice(),
            Host::Same(_) => &[],
            Host::Made(v) => v,
        }
    }
}

/// A device allocation, and the host mirror an ordinary array reads.
struct Resident {
    /// Identifies the backend the allocation belongs to. Two devices that
    /// share a backend share their uploads; a buffer from another one is
    /// not usable and is re-uploaded.
    device: usize,
    precision: Precision,
    elems: usize,
    handle: Handle,
    host: Host,
}

/// The device allocation behind this array's buffer, when it has one that
/// belongs to `backend` at `precision`.
fn resident_on<'a>(
    y: &'a Array,
    backend: &Arc<dyn Backend>,
    precision: Precision,
) -> Option<&'a Handle> {
    let owner = y.data.owner()?;
    let r: &Resident = owner.downcast_ref()?;
    let same = r.device == Arc::as_ptr(backend) as *const () as usize
        && r.precision == precision
        && r.elems == y.data.len();
    same.then_some(&r.handle)
}

/// A device operation that could not be carried out. These are host-side
/// failures — no adapter, an allocation refused, a shader the driver would
/// not compile — not language errors, and they never reach a program's
/// diagnostics: the caller sees them from [`Device::upload`], and a run
/// turns them into a fallback to the CPU.
#[derive(Clone, Debug)]
pub struct DeviceError(pub String);

impl std::fmt::Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DeviceError {}

/// An allocation on a device, opaque to everything but the backend that
/// made it.
pub(crate) struct Handle(pub Arc<dyn Any + Send + Sync>);

/// One dispatch: a generated shader, the buffers it reads, and the grid.
pub(crate) struct Plan<'a> {
    pub source: &'a str,
    pub entry: &'a str,
    pub inputs: &'a [&'a Handle],
    /// Elements the shader writes.
    pub out_elems: usize,
    pub elem_size: usize,
    /// Elements the kernel maps over.
    pub n: u32,
    /// Threads in the grid, for the grid-stride loop a reduction runs.
    pub stride: u32,
    pub groups: u32,
}

/// What a device backend must provide for the fused-kernel path.
///
/// One implementation, [`gpu`], covers Metal, Vulkan and DX12 through wgpu.
/// A second — CUDA, say — is another implementation of this trait and
/// nothing else: the kernel description, the code generator and the
/// placement rules above it are backend-agnostic.
pub(crate) trait Backend: Send + Sync + 'static {
    fn info(&self) -> &DeviceInfo;
    /// Copy elements into a device buffer, in the device's element type.
    fn upload(&self, values: &[f64], p: Precision) -> Result<Handle, DeviceError>;
    /// Compile (or reuse) the plan's shader and run it, returning what it
    /// wrote.
    fn dispatch(&self, plan: &Plan<'_>) -> Result<Vec<u8>, DeviceError>;
}

// --------------------------------------------------------------- placement

/// Why a fused node ran on the CPU although a device was asked for.
///
/// Every one of these is a statement about the kernel or its data, decided
/// before any work happens, except [`Failed`](Refusal::Failed), which is the
/// device itself refusing at run time. A fallback is always correct and only
/// ever slower.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The kernel's working type is i64. WGSL has no 64-bit integer
    /// arithmetic on most adapters, so integer chains stay on the CPU.
    Integer,
    /// The chain's result is not f64 — a comparison at the root, a tally.
    /// Narrowing a device result is not worth the risk this phase.
    NotFloat,
    /// The adapter has no f64 in shaders and the caller did not ask for
    /// f32. See the module note on precision.
    NoF64,
    /// The generator does not cover one of the chain's operations at this
    /// precision.
    Unsupported(&'static str),
    /// The kernel itself would decline these inputs, device or no device.
    Declined,
    /// Too little data to pay for a dispatch.
    TooSmall,
    /// The device refused: an allocation, a shader, a queue submission.
    Failed(String),
}

impl Refusal {
    pub fn reason(&self) -> String {
        match self {
            Refusal::Integer => "the chain computes in 64-bit integers".into(),
            Refusal::NotFloat => "the chain's result is not a float array".into(),
            Refusal::NoF64 => {
                "this adapter has no f64 in shaders; pass precision=\"f32\" to run anyway".into()
            }
            Refusal::Unsupported(op) => format!("`{op}` has no shader form here"),
            Refusal::Declined => "the fused kernel declined these inputs".into(),
            Refusal::TooSmall => "there is too little data to pay for a dispatch".into(),
            Refusal::Failed(e) => format!("the device refused: {e}"),
        }
    }
}

/// Where a fused node's arithmetic happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Placement {
    /// No device was asked for, so the question did not arise.
    Default,
    Gpu,
    /// The device would not take it, for this reason.
    Cpu(Refusal),
}

impl std::fmt::Display for Placement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Placement::Default => Ok(()),
            Placement::Gpu => write!(f, "device: gpu"),
            Placement::Cpu(why) => write!(f, "device: cpu ({})", why.reason()),
        }
    }
}

/// Least elements worth a dispatch.
///
/// Below this the round trip — two submissions, a queue wait, a readback —
/// costs more than the whole pass does on the CPU, whatever the arithmetic
/// per element is. Measured on a Radeon Pro 560 against the 8-thread CPU
/// path, the crossover for the simplest chain (`+/ w * x`) is around a
/// million elements; the threshold is set an octave below that so that a
/// heavier chain, which crosses over sooner, is not kept off the device.
pub const MIN_ELEMS: usize = 1 << 19;

/// Run a fused kernel on `device`, or say why it will not.
pub(crate) fn try_run(
    device: &Device,
    k: &FusedKernel,
    inputs: &[Array],
) -> Result<Array, Refusal> {
    let backend = device.backend().ok_or(Refusal::Declined)?;
    let precision = device.precision;
    if precision == Precision::F64 && !backend.info().f64 {
        return Err(Refusal::NoF64);
    }
    // A tally never touches values, and a reduction over one item is the
    // item itself: both are the fused path's own answers, exactly.
    if k.yields() == Yield::Tally {
        return Err(Refusal::Declined);
    }
    let Some(Some(shape)) = crate::fuse::common_shape(inputs) else {
        return Err(Refusal::Declined);
    };
    let n: usize = shape.iter().product();
    if n < MIN_ELEMS {
        return Err(Refusal::TooSmall);
    }
    let reducing = k.reduce().is_some();
    if reducing && shape.len() != 1 {
        return Err(Refusal::Declined);
    }
    let (working, root) = crate::fuse::working_type(k, inputs).ok_or(Refusal::Declined)?;
    if working != DType::F64 {
        return Err(Refusal::Integer);
    }
    if root != DType::F64 {
        return Err(Refusal::NotFloat);
    }

    // Every input either lies on the device already or goes up now. A
    // rank-0 input becomes a one-element buffer the shader reads at 0.
    let splat: Vec<bool> = inputs.iter().map(|a| a.rank() == 0).collect();
    let source = codegen::wgsl(k, &splat, precision).map_err(Refusal::Unsupported)?;

    let mut temporaries: Vec<Handle> = Vec::new();
    let mut slots: Vec<Option<&Handle>> = Vec::with_capacity(inputs.len());
    for a in inputs {
        match resident_on(a, backend, precision) {
            Some(h) => slots.push(Some(h)),
            None => {
                // A float argument goes up from its own buffer; only a
                // boolean or integer one is converted, and copying tens of
                // megabytes for nothing is exactly what that would be.
                let h = match &a.data {
                    Data::F64(v) => backend.upload(v.as_slice(), precision),
                    _ => backend.upload(&as_f64_vec(a), precision),
                }
                .map_err(|e| Refusal::Failed(e.0))?;
                temporaries.push(h);
                slots.push(None);
            }
        }
    }
    let mut next = 0usize;
    let buffers: Vec<&Handle> = slots
        .iter()
        .map(|s| match s {
            Some(h) => *h,
            None => {
                let h = &temporaries[next];
                next += 1;
                h
            }
        })
        .collect();

    let elem_size = precision.size();
    let out = if reducing {
        let groups = codegen::groups_for(n);
        let plan = Plan {
            source: &source,
            entry: codegen::REDUCE,
            inputs: &buffers,
            out_elems: groups,
            elem_size,
            n: n as u32,
            stride: (groups * codegen::WORKGROUP) as u32,
            groups: groups as u32,
        };
        let bytes = backend.dispatch(&plan).map_err(|e| Refusal::Failed(e.0))?;
        let partials = codegen::from_bytes(&bytes, precision, groups);
        // The partials combine right to left, as the CPU path's chunks do.
        // Only associative operations are absorbed, so this is the same
        // regrouping the float contract (§5.9) already allows.
        let op = k.reduce().expect("reducing");
        let mut acc = *partials.last().ok_or(Refusal::Declined)?;
        for &v in partials[..partials.len() - 1].iter().rev() {
            acc = crate::fuse::step(op, v, acc).ok_or(Refusal::Declined)?;
        }
        Array::scalar_f64(acc)
    } else {
        let plan = Plan {
            source: &source,
            entry: codegen::MAP,
            inputs: &buffers,
            out_elems: n,
            elem_size,
            n: n as u32,
            stride: 0,
            groups: n.div_ceil(codegen::WORKGROUP) as u32,
        };
        let bytes = backend.dispatch(&plan).map_err(|e| Refusal::Failed(e.0))?;
        let values = codegen::from_bytes(&bytes, precision, n);
        Array::new(shape, Data::F64(values.into()))
    };
    Ok(out)
}

fn as_f64_vec(a: &Array) -> Vec<f64> {
    match &a.data {
        Data::F64(v) => v.as_slice().to_vec(),
        Data::I64(v) => v.iter().map(|&x| x as f64).collect(),
        Data::Bool(v) => v.iter().map(|&x| x as f64).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cpu_is_always_a_device() {
        let d = Device::cpu();
        assert!(!d.is_gpu());
        assert!(d.info().is_none());
        let a = Array::from_f64(vec![1.0, 2.0]);
        assert_eq!(d.upload(&a).expect("cpu upload"), a);
    }

    #[test]
    fn every_refusal_says_something() {
        for r in [
            Refusal::Integer,
            Refusal::NotFloat,
            Refusal::NoF64,
            Refusal::Unsupported("^"),
            Refusal::Declined,
            Refusal::TooSmall,
            Refusal::Failed("no adapter".into()),
        ] {
            assert!(!r.reason().is_empty());
        }
    }
}
