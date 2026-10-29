//! ZFS dataset snapshot + clone isolation.
//!
//! When `lower` is the root of a ZFS filesystem dataset, [`start`] snapshots it
//! and clones the snapshot onto `merged` with an overridden `mountpoint`, so the
//! caller gets a zero-copy writable tree that diverges from `lower` only as it
//! is written. The CLI owns the pool-specific details; this backend only
//! locates the dataset backing `lower`, derives unique snapshot/dataset names,
//! and tears them down again on [`stop`](IsolationBackend::stop).
//!
//! Like [`btrfs`](crate::btrfs), `lower` has to *be* a dataset mountpoint: a
//! snapshot of an enclosing dataset would drag unrelated siblings along with
//! it. Datasets that contain child datasets are rejected for the same reason —
//! one snapshot+clone pair cannot reproduce nested mounts — and every such
//! rejection is [`IsoError::Unavailable`], so [`resolve`](crate::resolve)'s
//! caller can fall back instead of surfacing a hard failure.
//!
//! The clone is named `<dataset>/iso-<pid>-<nanos>-<seq>` and its origin
//! snapshot `<dataset>@iso-<pid>-<nanos>-<seq>`. That token is what lets
//! [`stop`](IsolationBackend::stop) recognise its own objects from `zfs list`
//! alone, so teardown still works after a process restart that lost in-memory
//! state — and it keeps the backend from destroying a dataset an operator
//! created at `merged` by hand.
//!
//! Unlike the other backends this has no `cfg(target_os = …)` split: it drives
//! the portable `zfs(8)` CLI through `std`, so the same code compiles and
//! type-checks everywhere and simply probes as unavailable where `OpenZFS` is
//! not installed (Windows, a Linux host without the module, …).

use std::path::Path;

use async_trait::async_trait;

use crate::{BackendKind, IsoResult, IsolationBackend, ProbeResult};

pub struct ZfsBackend;

pub fn backend() -> &'static dyn IsolationBackend {
    &ZfsBackend
}

#[async_trait]
impl IsolationBackend for ZfsBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Zfs
    }

    fn probe(&self) -> ProbeResult {
        imp::probe()
    }

    fn start(&self, lower: &Path, merged: &Path) -> IsoResult<()> {
        imp::start(lower, merged)
    }

    fn stop(&self, merged: &Path) -> IsoResult<()> {
        imp::stop(merged)
    }
}

mod imp {
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
        sync::atomic::{AtomicBool, AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{IsoError, IsoResult, ProbeResult, command_failed, tree};

    /// Prefix shared by every snapshot and clone dataset this backend creates.
    /// Teardown only touches objects that carry it.
    const TOKEN_PREFIX: &str = "iso-";

    /// Set once `zfs version` has run; a host that has it keeps it, so later
    /// probes skip the spawn. A failure is not cached: `OpenZFS` can be loaded
    /// while a long-running session is up, so the next probe spawns again.
    static CLI_AVAILABLE: AtomicBool = AtomicBool::new(false);

    /// Bumped for every token so two `start` calls inside one clock tick still
    /// derive different dataset names.
    static TOKEN_SEQ: AtomicU64 = AtomicU64::new(0);

    pub fn probe() -> ProbeResult {
        if CLI_AVAILABLE.load(Ordering::Relaxed) {
            return ProbeResult::available();
        }
        let probe = probe_cli();
        if probe.available {
            CLI_AVAILABLE.store(true, Ordering::Relaxed);
        }
        probe
    }

    fn probe_cli() -> ProbeResult {
        match Command::new("zfs")
            .arg("version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
        {
            Ok(output) if output.status.success() => ProbeResult::available(),
            Ok(output) => ProbeResult::unavailable(format!(
                "zfs CLI probe failed with exit {}: {}",
                output.status.code().unwrap_or(-1),
                command_message(&output.stderr, &output.stdout)
            )),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => ProbeResult::unavailable(
                "zfs CLI not found on PATH; ZFS dataset clone isolation needs OpenZFS",
            ),
            Err(err) => ProbeResult::unavailable(format!("unable to run the zfs CLI: {err}")),
        }
    }

    /// Snapshots the dataset rooted at `lower` and clones it onto `merged`.
    pub fn start(lower: &Path, merged: &Path) -> IsoResult<()> {
        // A bad source is reported as unavailable so the caller falls back
        // instead of surfacing a hard failure for a path ZFS cannot serve.
        let lower = tree::canonical_existing_dir(lower, "zfs source", IsoError::unavailable)?;
        let merged = std::path::absolute(merged).unwrap_or_else(|_| merged.to_path_buf());
        let dataset = dataset_for_mountpoint(&lower)?;
        ensure_no_child_datasets(&dataset)?;

        let token = unique_token();
        let snapshot = format!("{dataset}@{token}");
        let clone = format!("{dataset}/{token}");
        let mountpoint = format!("mountpoint={}", merged.display());

        tree::prepare_destination(&merged, "zfs clone", tree::remove_existing)?;
        fs::create_dir_all(&merged)
            .map_err(|err| IsoError::other(format!("create {}: {err}", merged.display())))?;

        zfs(&["snapshot", snapshot.as_str()])?;
        // `-o mountpoint` decouples where the clone lives in the pool from the
        // path the caller asked for, so `merged` holds `lower`'s contents and
        // nothing else.
        let cloned = zfs(&[
            "clone",
            "-o",
            mountpoint.as_str(),
            snapshot.as_str(),
            clone.as_str(),
        ]);
        if let Err(err) = cloned {
            // Never leak the snapshot when the clone is rejected.
            let _ = zfs(&["destroy", "-d", snapshot.as_str()]);
            let _ = fs::remove_dir(&merged);
            return Err(err);
        }
        Ok(())
    }

    /// Destroys the clone mounted at `merged` and the snapshot it came from.
    pub fn stop(merged: &Path) -> IsoResult<()> {
        let merged = std::path::absolute(merged).unwrap_or_else(|_| merged.to_path_buf());
        // Rediscover our objects from `zfs list`: teardown has to keep working
        // after a restart that lost in-memory state.
        match clone_for_mountpoint(&merged) {
            Ok(Some((clone, snapshot))) => {
                match zfs(&["destroy", "-rf", clone.as_str()]) {
                    Ok(_) => {}
                    // Another process already tore the clone down.
                    Err(err) if is_missing_object(err.message()) => {}
                    Err(err) => return Err(err),
                }
                // `-d` defers the destroy while anything still references the
                // snapshot instead of failing outright.
                let _ = zfs(&["destroy", "-d", snapshot.as_str()]);
            }
            // Not a ZFS mountpoint, or not one of ours: only the leftover
            // directory below can still be ours to remove.
            Ok(None) => {}
            Err(err) => return Err(err),
        }
        remove_mountpoint_dir(&merged)
    }

    /// The clone dataset this backend mounted at `merged`, with the snapshot it
    /// was cloned from. `Ok(None)` means "nothing of ours is mounted there".
    fn clone_for_mountpoint(merged: &Path) -> IsoResult<Option<(String, String)>> {
        let dataset = match dataset_for_mountpoint(merged) {
            Ok(dataset) => dataset,
            Err(err) if err.is_unavailable() => return Ok(None),
            Err(err) => return Err(err),
        };
        let Some((parent, token)) = dataset.rsplit_once('/') else {
            return Ok(None);
        };
        // Only `<parent>/iso-<pid>-<nanos>-<seq>` clones are ours.
        if !token.starts_with(TOKEN_PREFIX) {
            return Ok(None);
        }
        let snapshot = format!("{parent}@{token}");
        Ok(Some((dataset, snapshot)))
    }

    /// Name of the ZFS filesystem dataset whose mountpoint is exactly `path`.
    fn dataset_for_mountpoint(path: &Path) -> IsoResult<String> {
        let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let listing = zfs(&[
            "list",
            "-H",
            "-p",
            "-o",
            "name,mountpoint",
            "-t",
            "filesystem",
        ])?;
        for line in listing.lines() {
            let Some((name, mountpoint)) = line.split_once('\t') else {
                continue;
            };
            if mountpoint_equals(mountpoint, &path) {
                return Ok(name.trim().to_owned());
            }
        }
        Err(IsoError::unavailable(format!(
            "{} is not the root of a ZFS dataset; a dataset snapshot only reproduces \
             the mountpoint itself",
            path.display()
        )))
    }

    /// Rejects datasets with child datasets: a single snapshot+clone pair
    /// cannot reproduce nested mounts, and silently dropping them would hand
    /// the workload an incomplete tree.
    fn ensure_no_child_datasets(dataset: &str) -> IsoResult<()> {
        let listing = zfs(&[
            "list",
            "-H",
            "-o",
            "name",
            "-r",
            "-d",
            "1",
            "-t",
            "filesystem,volume",
            dataset,
        ])?;
        let datasets = listing
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        if datasets > 1 {
            return Err(IsoError::unavailable(format!(
                "dataset {dataset} has child datasets; a single clone cannot capture them"
            )));
        }
        Ok(())
    }

    /// Runs `zfs <args>`, returning trimmed stdout on success.
    ///
    /// Failures the host cannot fix by retrying — missing privileges, no ZFS at
    /// the path, an unloadable module — surface as [`IsoError::Unavailable`] so
    /// the caller can fall back to another backend; everything else is a hard
    /// [`IsoError::Other`].
    fn zfs(args: &[&str]) -> IsoResult<String> {
        let output = Command::new("zfs")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|err| spawn_error(args, &err))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
        }
        let what = format!("zfs {}", args.join(" "));
        let message = command_message(&output.stderr, &output.stdout);
        if is_unavailable_failure(&message) {
            Err(IsoError::unavailable(format!("{what}: {message}")))
        } else {
            Err(command_failed(
                what,
                output.status.code().unwrap_or(-1),
                &output.stderr,
            ))
        }
    }

    /// Classifies a failed `zfs` spawn: a missing binary is the one case a
    /// caller can recover from by falling back.
    fn spawn_error(args: &[&str], err: &std::io::Error) -> IsoError {
        if err.kind() == std::io::ErrorKind::NotFound {
            IsoError::unavailable(
                "zfs CLI not found on PATH; ZFS dataset clone isolation needs OpenZFS",
            )
        } else {
            IsoError::other(format!("spawn zfs {}: {err}", args.join(" ")))
        }
    }

    /// Prefer stderr, fall back to stdout, and never return an empty message.
    fn command_message(stderr: &[u8], stdout: &[u8]) -> String {
        let stderr = String::from_utf8_lossy(stderr);
        let stderr = stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_owned();
        }
        let stdout = String::from_utf8_lossy(stdout);
        let stdout = stdout.trim();
        if stdout.is_empty() {
            "no command output".to_string()
        } else {
            stdout.to_owned()
        }
    }

    /// Whether a `mountpoint` property value denotes `path`.
    ///
    /// `none`, `legacy`, and `-` mean "not mounted at a fixed path", and
    /// relative values are resolved by ZFS against the parent dataset, which we
    /// cannot reproduce here; both count as "no match" so the caller falls back.
    fn mountpoint_equals(mountpoint: &str, path: &Path) -> bool {
        let mountpoint = mountpoint.trim();
        let candidate = Path::new(mountpoint);
        if mountpoint.is_empty() || !candidate.is_absolute() {
            return false;
        }
        fs::canonicalize(candidate).is_ok_and(|resolved| resolved == path)
    }

    /// Failure messages that mean "this host cannot do ZFS isolation" rather
    /// than "the operation broke".
    fn is_unavailable_failure(message: &str) -> bool {
        let message = message.to_ascii_lowercase();
        [
            "permission denied",
            "must be root",
            "insufficient privileges",
            "operation not permitted",
            "no such pool",
            "no such pool or dataset",
            "not a zfs filesystem",
            "cannot mount",
            "mount failed",
            "pool is unavailable",
            "zfs modules are not loaded",
            "module not loaded",
        ]
        .iter()
        .any(|needle| message.contains(needle))
    }

    /// Failure messages that mean the object was already removed.
    fn is_missing_object(message: &str) -> bool {
        let message = message.to_ascii_lowercase();
        message.contains("does not exist") || message.contains("no such dataset")
    }

    /// Snapshot/clone name shared by one `start` call.
    fn unique_token() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let pid = std::process::id();
        let seq = TOKEN_SEQ.fetch_add(1, Ordering::Relaxed);
        format!("{TOKEN_PREFIX}{pid}-{nanos}-{seq}")
    }

    /// Removes the now-empty mountpoint directory.
    ///
    /// Only a non-recursive remove is attempted: when ZFS is unavailable or the
    /// clone could not be destroyed, `merged` may still hold the clone's
    /// contents, and deleting those would destroy data we do not own.
    fn remove_mountpoint_dir(path: &Path) -> IsoResult<()> {
        match fs::remove_dir(path) {
            Ok(()) => Ok(()),
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                Ok(())
            }
            Err(err) => Err(IsoError::other(format!(
                "remove mountpoint {}: {err}",
                path.display()
            ))),
        }
    }

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use super::*;

        /// A scratch directory under the system temp dir. Cleanup is
        /// best-effort: a leftover directory in temp is harmless.
        fn scratch(tag: &str) -> PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "pi-iso-zfs-{tag}-{}-{}",
                std::process::id(),
                TOKEN_SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).expect("create scratch directory");
            dir
        }

        #[test]
        fn host_level_rejections_are_reported_as_unavailable() {
            for message in [
                "cannot open 'pool/data': permission denied",
                "must be root to perform this operation",
                "cannot open 'pool/data': no such pool or dataset",
                "cannot mount '/tmp/x': directory is not empty",
                "the ZFS modules are not loaded.",
            ] {
                assert!(
                    is_unavailable_failure(message),
                    "not unavailable: {message}"
                );
            }
            // Real breakage has to stay a hard error so the caller sees it.
            for message in [
                "cannot create snapshot 'pool/data@iso-1': out of space",
                "dataset already exists",
            ] {
                assert!(
                    !is_unavailable_failure(message),
                    "wrongly unavailable: {message}"
                );
            }
        }

        #[test]
        fn already_removed_objects_keep_teardown_idempotent() {
            assert!(is_missing_object(
                "cannot open 'pool/data/iso-1-2-3': dataset does not exist"
            ));
            assert!(is_missing_object("no such dataset: pool@snap"));
            assert!(!is_missing_object(
                "cannot destroy 'pool/x': permission denied"
            ));
        }

        #[test]
        fn mountpoint_values_match_only_real_dataset_roots() {
            let root = fs::canonicalize(scratch("mountpoint")).expect("canonicalize scratch");
            let as_mountpoint = root.to_string_lossy().into_owned();

            assert!(mountpoint_equals(&as_mountpoint, &root));
            assert!(!mountpoint_equals("none", &root));
            assert!(!mountpoint_equals("legacy", &root));
            assert!(!mountpoint_equals("-", &root));
            assert!(!mountpoint_equals("", &root));
            assert!(!mountpoint_equals("relative", &root));
            assert!(!mountpoint_equals("/", &root));

            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn tokens_are_unique_and_safe_as_dataset_components() {
            let first = unique_token();
            let second = unique_token();

            assert!(first.starts_with(TOKEN_PREFIX));
            assert_ne!(first, second);
            assert!(!first.contains('@'));
            assert!(!first.contains('/'));
            assert!(!first.contains(' '));
        }

        /// Without the CLI a start has to fail as `Unavailable` (so `resolve`
        /// falls back to `rcopy`) and must not leave the destination behind.
        #[test]
        fn start_without_zfs_falls_back_and_cleans_up() {
            if probe().available {
                // A host that really has ZFS needs a pool to exercise this.
                return;
            }
            let root = scratch("start");
            let merged = root.join("merged");

            let err = start(&root, &merged).expect_err("start must reject a non-dataset source");

            assert!(
                err.is_unavailable(),
                "expected a fallback error, got {err:?}"
            );
            assert!(!merged.exists(), "failed start must not create {merged:?}");
            let _ = fs::remove_dir_all(&root);
        }
    }
}
