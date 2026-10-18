//! OS-reported system thermal state, independent of individual temperature sensors.

use std::ffi::{CStr, c_char, c_void};

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use serde::Serialize;

/// System thermal pressure reported by `NSProcessInfo.thermalState`.
///
/// This is not a temperature threshold or proof of CPU/GPU throttling. Apple also
/// reports `Nominal` when the platform's thermal state is unknown or unsupported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "app", derive(serde::Deserialize))]
#[serde(rename_all = "lowercase")]
pub enum ThermalState {
  /// The OS reports that no corrective action is needed.
  Nominal,
  /// The OS recommends deferring nonessential work.
  Fair,
  /// The OS recommends reducing CPU, GPU and I/O activity.
  Serious,
  /// The OS recommends minimizing activity so the system can cool down.
  Critical,
}

impl ThermalState {
  fn from_raw(value: isize) -> Option<Self> {
    match value {
      0 => Some(Self::Nominal),
      1 => Some(Self::Fair),
      2 => Some(Self::Serious),
      3 => Some(Self::Critical),
      _ => None,
    }
  }
}

type Id = *mut c_void;
type Sel = *mut c_void;

// A real Foundation symbol keeps the framework linked; CFString and NSString are bridged.
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
  fn NSClassFromString(name: CFStringRef) -> Id;
}

#[link(name = "objc")]
unsafe extern "C" {
  fn sel_registerName(name: *const c_char) -> Sel;
  fn class_getClassMethod(class: Id, selector: Sel) -> *mut c_void;
  fn class_getInstanceMethod(class: Id, selector: Sel) -> *mut c_void;
  fn objc_msgSend();
}

fn class_methods(name: &str, factory: &CStr, method: &CStr) -> Option<(Id, Sel, Sel)> {
  let name = CFString::new(name);
  unsafe {
    let class = NSClassFromString(name.as_concrete_TypeRef());
    let factory = sel_registerName(factory.as_ptr());
    let method = sel_registerName(method.as_ptr());
    if class.is_null()
      || class_getClassMethod(class, factory).is_null()
      || class_getInstanceMethod(class, method).is_null()
    {
      return None;
    }
    Some((class, factory, method))
  }
}

unsafe fn send_id(receiver: Id, selector: Sel) -> Id {
  let send: unsafe extern "C" fn(Id, Sel) -> Id =
    unsafe { std::mem::transmute(objc_msgSend as *const ()) };
  unsafe { send(receiver, selector) }
}

/// The Foundation objects must be autoreleased on the sampler's worker thread, too.
struct Pool {
  object: Id,
  drain: Sel,
}

impl Pool {
  fn new() -> Option<Self> {
    let (class, new, drain) = class_methods("NSAutoreleasePool", c"new", c"drain")?;
    let object = unsafe { send_id(class, new) };
    (!object.is_null()).then_some(Self { object, drain })
  }
}

impl Drop for Pool {
  fn drop(&mut self) {
    unsafe {
      let drain: unsafe extern "C" fn(Id, Sel) = std::mem::transmute(objc_msgSend as *const ());
      drain(self.object, self.drain);
    }
  }
}

pub(crate) fn read_thermal_state(vm: bool) -> Option<ThermalState> {
  // A VM's nominal fallback does not establish the host's physical thermal condition.
  if vm {
    return None;
  }
  let _pool = Pool::new()?;
  let (class, factory, selector) = class_methods("NSProcessInfo", c"processInfo", c"thermalState")?;
  let process = unsafe { send_id(class, factory) };
  if process.is_null() {
    return None;
  }
  // NSProcessInfoThermalState is NSInteger, not an object pointer or an unsigned byte.
  let value = unsafe {
    let send: unsafe extern "C" fn(Id, Sel) -> isize =
      std::mem::transmute(objc_msgSend as *const ());
    send(process, selector)
  };
  ThermalState::from_raw(value)
}

#[cfg(test)]
mod tests {
  use super::{ThermalState, class_methods, read_thermal_state};

  #[test]
  fn known_thermal_values_serialize_as_the_os_states_without_temperature_thresholds() {
    for (raw, state, name) in [
      (0, ThermalState::Nominal, "nominal"),
      (1, ThermalState::Fair, "fair"),
      (2, ThermalState::Serious, "serious"),
      (3, ThermalState::Critical, "critical"),
    ] {
      assert_eq!(ThermalState::from_raw(raw), Some(state));
      assert_eq!(serde_json::to_value(state).unwrap(), name);
    }
    for raw in [isize::MIN, -1, 4, 256, isize::MAX] {
      assert_eq!(ThermalState::from_raw(raw), None);
    }
    assert_eq!(serde_json::to_value(None::<ThermalState>).unwrap(), serde_json::Value::Null);
  }

  #[test]
  fn missing_classes_and_selectors_are_rejected_before_sending_messages() {
    for (name, factory, method) in [
      ("MacmonMissingThermalStateClass", c"processInfo", c"thermalState"),
      ("NSProcessInfo", c"macmonMissingThermalFactory", c"thermalState"),
      ("NSProcessInfo", c"processInfo", c"macmonMissingThermalState"),
    ] {
      assert!(class_methods(name, factory, method).is_none());
    }
  }

  #[test]
  fn virtual_machines_leave_the_host_thermal_state_unavailable() {
    assert_eq!(read_thermal_state(true), None);
  }

  #[test]
  #[ignore = "opt-in read-only property check on a supported physical Mac"]
  fn public_thermal_property_read_only_hardware_probe() {
    // No load is generated: read the public property to exercise the typed ObjC ABI and pool.
    for _ in 0..8 {
      assert!(read_thermal_state(false).is_some());
    }
  }
}
