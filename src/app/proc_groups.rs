//! App-only process ownership and totals. Paths and UIDs never enter serialized groups.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, Hasher};
use std::path::{Component, Path};

use serde::Serialize;

use crate::procs::ProcInfo;

#[derive(Default, Clone, Debug, PartialEq)]
pub struct ProcMeta {
  pub instance: String,
  pub app: Option<AppIdentity>,
  pub known: (bool, bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppIdentity {
  pub id: String,
  pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupKind {
  Application,
  Process,
}

#[derive(Clone, Debug, Serialize)]
pub struct GroupMember {
  pub pid: i32,
  pub instance: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProcGroup {
  pub id: String,
  pub name: String,
  pub kind: GroupKind,
  pub members: Vec<GroupMember>,
  pub cpu_pct: Option<f32>,
  pub gpu_pct: Option<f32>,
  pub cpu_known: usize,
  pub gpu_known: usize,
  pub mem_bytes: Option<u64>,
  #[serde(skip)]
  pub power_w: Option<f32>,
  #[serde(skip)]
  pub power_known: usize,
  #[serde(skip)]
  pub cpu_time_ns: Option<u64>,
}

pub(crate) fn measured(value: f32) -> Option<f32> {
  (value.is_finite() && value >= 0.0).then_some(value)
}

/// Totals always use the complete snapshot; filtering is a presentation operation.
pub fn groups(rows: &[ProcInfo]) -> Vec<ProcGroup> {
  let mut seen = HashSet::new();
  let mut totals = HashMap::<String, (ProcGroup, [f64; 3])>::new();
  for row in rows.iter().filter(|row| seen.insert(row.pid)) {
    let (id, name, kind) = match &row.meta.app {
      Some(app) => (app.id.clone(), app.name.clone(), GroupKind::Application),
      None => (row.meta.instance.clone(), row.name.clone(), GroupKind::Process),
    };
    let id = if id.is_empty() { format!("process-{}", row.pid) } else { id };
    let (group, sums) = totals.entry(id.clone()).or_insert_with(|| {
      (
        ProcGroup {
          id,
          name,
          kind,
          members: Vec::new(),
          cpu_pct: None,
          gpu_pct: None,
          cpu_known: 0,
          gpu_known: 0,
          mem_bytes: Some(0),
          power_w: None,
          power_known: 0,
          cpu_time_ns: Some(0),
        },
        [0.0; 3],
      )
    });
    group.members.push(GroupMember { pid: row.pid, instance: row.meta.instance.clone() });
    group.mem_bytes = group.mem_bytes.and_then(|sum| sum.checked_add(row.mem_bytes));
    group.cpu_time_ns = group.cpu_time_ns.and_then(|sum| sum.checked_add(row.cpu_time_ns));
    for (index, value, known) in [
      (0, row.meta.known.0.then_some(row.cpu_pct), &mut group.cpu_known),
      (1, row.meta.known.1.then_some(row.gpu_pct), &mut group.gpu_known),
      (2, row.power_w.filter(|_| row.meta.known.0), &mut group.power_known),
    ] {
      if let Some(value) = value.and_then(measured) {
        sums[index] += f64::from(value);
        *known += 1;
      }
    }
  }
  let mut groups: Vec<_> = totals
    .into_values()
    .map(|(mut group, sums)| {
      let rate = |index, known| (known > 0).then(|| sums[index] as f32).and_then(measured);
      group.cpu_pct = rate(0, group.cpu_known);
      group.gpu_pct = rate(1, group.gpu_known);
      group.power_w = rate(2, group.power_known);
      group.members.sort_by_key(|member| member.pid);
      group
    })
    .collect();
  groups.sort_by(|a, b| a.id.cmp(&b.id));
  groups
}

// Optional private ABI, not SDK API. XNU xnu-11417.140.69:
// https://github.com/apple-oss-distributions/xnu/blob/xnu-11417.140.69/bsd/sys/proc_info_private.h
// proc_uniqidentifierinfo is 56 bytes; proc_pidcoalitioninfo is 40 bytes.
// proc_info.c exempts both flavors from same-user checks, but MAC policy can still deny them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Identity {
  pub unique: u64,
  pub version: i32,
}

#[repr(C)]
#[derive(Default)]
struct NativeIdentity {
  uuid: [u8; 16],
  unique: u64,
  parent: u64,
  version: i32,
  parent_version: i32,
  reserved: [u64; 2],
}

#[repr(C)]
#[derive(Default)]
struct NativeCoalition {
  ids: [u64; 2],
  reserved: [u64; 3],
}

fn read_info<T: Default>(
  pid: i32,
  read: impl FnOnce(*mut std::ffi::c_void, i32) -> i32,
) -> Option<T> {
  if pid <= 0 {
    return None;
  }
  let mut info = T::default();
  let size = std::mem::size_of::<T>() as i32;
  let count = read((&raw mut info).cast(), size);
  (count == size).then_some(info)
}

fn native_info<T: Default>(pid: i32, flavor: i32) -> Option<T> {
  read_info(pid, |buffer, size| unsafe { libc::proc_pidinfo(pid, flavor, 0, buffer, size) })
}

pub(crate) fn identity(pid: i32) -> Option<Identity> {
  let native: NativeIdentity = native_info(pid, 17)?;
  (native.unique != 0).then_some(Identity { unique: native.unique, version: native.version })
}

pub(crate) type Coalition = [u64; 2];

pub(crate) fn coalition(pid: i32) -> Option<Coalition> {
  let native: NativeCoalition = native_info(pid, 20)?;
  native.ids.iter().all(|id| *id != 0).then_some(native.ids)
}

/// Identity must bracket UID/counter/path reads. Missing ABI support keeps an unowned row.
pub(crate) fn consistent(
  before: Option<Identity>,
  after: Option<Identity>,
) -> Result<Option<Identity>, ()> {
  match (before, after) {
    (Some(a), Some(b)) if a != b => Err(()),
    (Some(a), Some(_)) => Ok(Some(a)),
    _ => Ok(None),
  }
}

#[derive(Clone)]
struct Bundle {
  app: Option<(String, String)>,
  xpc: bool,
}

fn classify(path: &str) -> Bundle {
  let path = Path::new(path);
  if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
    return Bundle { app: None, xpc: false };
  }
  let enclosing = |suffix| {
    path.ancestors().filter(move |bundle| {
      bundle.extension().is_some_and(|ext| ext == suffix)
        && path.strip_prefix(bundle).ok().and_then(|rest| rest.components().next())
          == Some(Component::Normal("Contents".as_ref()))
    })
  };
  let app = enclosing("app").last().map(|bundle| {
    (
      bundle.to_string_lossy().into_owned(),
      bundle.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
    )
  });
  Bundle { app, xpc: enclosing("xpc").next().is_some() }
}

/// Cache only live package paths; app IDs survive while any observed member survives.
pub(crate) struct Ownership {
  salt: u64,
  next: u64,
  default: Option<Coalition>,
  paths: HashMap<String, Bundle>,
  apps: HashMap<(u32, String), AppIdentity>,
}

impl Ownership {
  pub fn new() -> Self {
    Self {
      salt: std::collections::hash_map::RandomState::new().build_hasher().finish(),
      next: 0,
      default: coalition(1),
      paths: HashMap::new(),
      apps: HashMap::new(),
    }
  }

  pub fn fresh(&mut self) -> String {
    self.next += 1;
    format!("{:016x}-{:x}", self.salt, self.next)
  }

  pub fn candidate(&mut self, path: &str) -> bool {
    let bundle = self.paths.entry(path.into()).or_insert_with(|| classify(path));
    bundle.app.is_some() || bundle.xpc
  }

  pub fn applications(
    &mut self,
    entries: &[(u32, Option<&str>, Option<Coalition>)],
  ) -> Vec<Option<AppIdentity>> {
    let live: HashSet<_> = entries.iter().filter_map(|(_, path, _)| *path).collect();
    self.paths.retain(|path, _| live.contains(path.as_str()));
    let mut active = HashSet::new();
    let mut owners = HashMap::<(u32, Coalition), Option<AppIdentity>>::new();
    let mut result = Vec::with_capacity(entries.len());
    for &(uid, path, coal) in entries {
      let bundle =
        path.map(|path| self.paths.entry(path.into()).or_insert_with(|| classify(path)).clone());
      let app = bundle.as_ref().and_then(|bundle| bundle.app.as_ref()).map(|(path, name)| {
        let key = (uid, path.clone());
        active.insert(key.clone());
        if !self.apps.contains_key(&key) {
          let id = self.fresh();
          self.apps.insert(key.clone(), AppIdentity { id, name: name.clone() });
        }
        self.apps[&key].clone()
      });
      if let (Some(app), Some(coal)) = (
        &app,
        coal.filter(|coal| {
          coal.iter().all(|id| *id != 0) && self.default.is_some_and(|default| default != *coal)
        }),
      ) {
        owners
          .entry((uid, coal))
          .and_modify(|owner| {
            if owner.as_ref() != Some(app) {
              *owner = None;
            }
          })
          .or_insert_with(|| Some(app.clone()));
      }
      result.push((app, bundle.is_some_and(|bundle| bundle.xpc)));
    }
    self.apps.retain(|key, _| active.contains(key));
    result
      .into_iter()
      .zip(entries)
      .map(|((app, xpc), &(uid, _, coal))| {
        app.or_else(|| {
          coal.filter(|_| xpc).and_then(|coal| owners.get(&(uid, coal)).cloned().flatten())
        })
      })
      .collect()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const APP: &str = "/Applications/Browser.app/Contents/MacOS/Browser";
  const HELPER: &str =
    "/Applications/Browser.app/Contents/Frameworks/Renderer.app/Contents/MacOS/worker";
  const XPC: &str =
    "/System/Library/Frameworks/WebKit.framework/XPCServices/web.xpc/Contents/MacOS/service";
  const COAL: Coalition = [30, 31];

  fn ownership() -> Ownership {
    Ownership { default: Some([1, 3]), ..Ownership::new() }
  }

  fn row(pid: i32, instance: &str, app: Option<AppIdentity>, known: (bool, bool)) -> ProcInfo {
    ProcInfo {
      pid,
      name: "worker".into(),
      path: APP.into(),
      user: "private-user".into(),
      cpu_pct: 150.0,
      cpu_time_ns: 40,
      mem_bytes: 64,
      power_w: Some(2.0),
      gpu_pct: 75.0,
      started: Some((42, 0)),
      meta: ProcMeta { instance: instance.into(), app, known },
    }
  }

  #[test]
  fn strict_optional_abi_layout_and_short_reads() {
    assert_eq!(std::mem::size_of::<NativeIdentity>(), 56);
    assert_eq!(std::mem::offset_of!(NativeIdentity, unique), 16);
    assert_eq!(std::mem::offset_of!(NativeIdentity, version), 32);
    assert_eq!(std::mem::size_of::<NativeCoalition>(), 40);
    assert!(read_info::<NativeIdentity>(0, |_, _| panic!("do not query kernel PID")).is_none());
    for count in [-1, 0, 39, 41] {
      assert!(
        read_info::<NativeCoalition>(42, |_, size| {
          assert_eq!(size, 40);
          count
        })
        .is_none()
      );
    }
    for count in [-1, 0, 55, 57] {
      assert!(
        read_info::<NativeIdentity>(42, |_, size| {
          assert_eq!(size, 56);
          count
        })
        .is_none()
      );
    }
    let native = read_info::<NativeCoalition>(42, |buffer, size| {
      unsafe {
        buffer.cast::<NativeCoalition>().write(NativeCoalition { ids: COAL, ..Default::default() });
      }
      size
    })
    .unwrap();
    assert_eq!(native.ids, COAL);
  }

  #[test]
  fn pid_reuse_exec_and_missing_identity_never_validate_an_old_observation() {
    let first = Identity { unique: 7, version: 11 };
    assert_eq!(consistent(Some(first), Some(first)), Ok(Some(first)));
    for changed in [Identity { unique: 8, ..first }, Identity { version: 12, ..first }] {
      assert_eq!(consistent(Some(first), Some(changed)), Err(()));
    }
    for (before, after) in [(None, None), (Some(first), None), (None, Some(first))] {
      assert_eq!(consistent(before, after), Ok(None));
    }
  }

  #[test]
  fn bundle_components_and_nested_helpers_do_not_use_process_names() {
    assert_eq!(classify(APP).app, classify(HELPER).app);
    assert_eq!(classify(HELPER).app.unwrap().1, "Browser");
    assert!(classify(XPC).xpc);
    for path in [
      "relative.app/Contents/MacOS/x",
      "/tmp/a.app/../cargo",
      "/tmp/a.app-cache/Contents/x",
      "/tmp/a.app/tools/x",
      "/bin/zsh",
    ] {
      assert!(classify(path).app.is_none(), "{path}");
    }
    let mut owner = ownership();
    let apps = owner.applications(&[
      (501, Some(APP), Some(COAL)),
      (501, Some(HELPER), Some(COAL)),
      (501, Some(XPC), Some(COAL)),
      (501, Some("/bin/zsh"), Some(COAL)),
      (501, Some("/opt/bin/cargo"), Some(COAL)),
    ]);
    assert!(apps[0].is_some());
    assert_eq!(apps[0], apps[1]);
    assert_eq!(apps[0], apps[2]);
    assert!(apps[3].is_none() && apps[4].is_none());
  }

  #[test]
  fn xpc_requires_a_live_unique_same_user_full_nondefault_pair() {
    let other = "/Applications/Other.app/Contents/MacOS/Other";
    let mut owner = ownership();
    for entries in [
      vec![(501, Some(APP), Some(COAL)), (502, Some(XPC), Some(COAL))],
      vec![(501, Some(APP), Some(COAL)), (501, Some(XPC), Some([30, 99]))],
      vec![(501, Some(APP), Some([0, 31])), (501, Some(XPC), Some([0, 31]))],
      vec![(501, Some(APP), Some([1, 3])), (501, Some(XPC), Some([1, 3]))],
      vec![(501, Some(APP), None), (501, Some(XPC), Some(COAL))],
      vec![
        (501, Some(APP), Some(COAL)),
        (501, Some(other), Some(COAL)),
        (501, Some(XPC), Some(COAL)),
      ],
      vec![(501, None, Some(COAL)), (501, Some(XPC), Some(COAL))],
    ] {
      assert!(owner.applications(&entries).last().unwrap().is_none());
    }
    assert!(owner.applications(&[(501, Some(XPC), Some(COAL))])[0].is_none());
    owner.default = None;
    assert!(
      owner.applications(&[(501, Some(APP), Some(COAL)), (501, Some(XPC), Some(COAL))])[1]
        .is_none()
    );
  }

  #[test]
  fn ownership_ids_are_opaque_stable_separate_users_and_copies_and_prune() {
    let mut owner = ownership();
    let first = owner.applications(&[(501, Some(APP), None)])[0].clone().unwrap();
    assert!(!first.id.contains("Browser") && !first.id.contains('/'));
    assert_eq!(owner.applications(&[(501, Some(HELPER), None)])[0], Some(first.clone()));
    let distinct = owner.applications(&[
      (501, Some(APP), None),
      (502, Some(APP), None),
      (501, Some("/Volumes/Copy/Browser.app/Contents/MacOS/Browser"), None),
    ]);
    assert_ne!(distinct[0], distinct[1]);
    assert_ne!(distinct[0], distinct[2]);
    assert!(owner.applications(&[(501, None, None)])[0].is_none());
    assert!(owner.apps.is_empty() && owner.paths.is_empty());
    assert_ne!(owner.applications(&[(501, Some(APP), None)])[0], Some(first));
  }

  #[test]
  fn complete_totals_preserve_partial_and_measured_zero_without_clamping_cpu() {
    let app = AppIdentity { id: "opaque-app".into(), name: "Browser".into() };
    let mut rows =
      vec![row(2, "a", Some(app.clone()), (true, true)), row(3, "b", Some(app), (true, false))];
    rows[1].gpu_pct = 0.0;
    rows[1].power_w = None;
    let totals = groups(&rows);
    let total = &totals[0];
    assert_eq!(
      (total.cpu_pct, total.gpu_pct, total.mem_bytes),
      (Some(300.0), Some(75.0), Some(128))
    );
    assert_eq!((total.cpu_known, total.gpu_known, total.power_known), (2, 1, 1));
    assert_eq!((total.power_w, total.cpu_time_ns), (Some(2.0), Some(80)));
    assert_eq!(total.kind, GroupKind::Application);
    assert_eq!(total.members.iter().map(|member| member.pid).collect::<Vec<_>>(), [2, 3]);
    // Filtering can select this group without changing its already-computed full totals.
    assert_eq!(
      totals
        .iter()
        .find(|group| group.members.iter().any(|member| member.pid == 3))
        .unwrap()
        .cpu_pct,
      Some(300.0)
    );
    rows[0].cpu_pct = 0.0;
    rows[1].meta.known.0 = false;
    assert_eq!(groups(&rows)[0].cpu_pct, Some(0.0));
    rows[0].meta.known = (false, false);
    assert_eq!((groups(&rows)[0].cpu_pct, groups(&rows)[0].gpu_pct), (None, None));
  }

  #[test]
  fn unique_members_finite_rates_and_checked_integer_totals() {
    let app = AppIdentity { id: "app".into(), name: "app".into() };
    let mut first = row(1, "first", Some(app.clone()), (true, true));
    first.mem_bytes = u64::MAX;
    first.cpu_time_ns = u64::MAX;
    first.cpu_pct = f32::NAN;
    first.gpu_pct = -1.0;
    let mut second = row(2, "second", Some(app), (true, true));
    second.cpu_pct = f32::INFINITY;
    second.gpu_pct = 0.0;
    let grouped = groups(&[first.clone(), second.clone(), second]);
    let group = &grouped[0];
    assert_eq!(group.members.len(), 2);
    assert_eq!(
      (group.cpu_pct, group.cpu_known, group.gpu_pct, group.gpu_known),
      (None, 0, Some(0.0), 1)
    );
    assert_eq!((group.mem_bytes, group.cpu_time_ns), (None, None));
    let json = serde_json::to_value(group).unwrap();
    assert_eq!(json["kind"], "application");
    for private in ["power_w", "power_known", "cpu_time_ns", "path", "uid", "user"] {
      assert!(json.get(private).is_none(), "{private}");
    }
    let standalone =
      groups(&[row(4, "one", None, (false, false)), row(5, "two", None, (false, false))]);
    assert_eq!(standalone.len(), 2);
    assert!(standalone.iter().all(|group| group.kind == GroupKind::Process));
    assert_eq!(groups(&[first.clone(), first])[0].mem_bytes, Some(u64::MAX));
  }
}
