use std::io;
use std::sync::Arc;

use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use test_case::test_case;
use tokio::sync::Semaphore;

use super::EffectGuard;
use super::MutationLease;
use super::test_support::Pause;
use super::test_support::Phase;
use super::test_support::available_permits;
use crate::ConditionalWriteResult;
use crate::CopyOptions;
use crate::ExecutorFileSystem;
use crate::LocalFileSystem;
use crate::RemoveOptions;
use crate::WriteFileOptions;
use crate::local_file_system::DirectFileSystem;

#[tokio::test]
async fn uncertain_effect_closes_owner_before_releasing_lease() {
    let owner = Arc::new(Semaphore::new(/*permits*/ 1));
    let permit = Arc::clone(&owner).acquire_owned().await.expect("lease");
    let lease = MutationLease {
        effect: EffectGuard {
            owner: Arc::clone(&owner),
            settled: false,
        },
        _permit: permit,
    };
    drop(lease);
    assert!(owner.is_closed());
    assert!(owner.try_acquire().is_err());
    let owner = Arc::new(Semaphore::new(/*permits*/ 1));
    let mut effect = EffectGuard {
        owner: Arc::clone(&owner),
        settled: false,
    };
    effect.settle();
    drop(effect);
    assert!(!owner.is_closed());
    assert_eq!(owner.available_permits(), 1);
}

#[tokio::test]
async fn unsupported_filesystem_does_not_fall_back_to_an_ordinary_write() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical temporary root");
    let path = root.join("file");
    std::fs::write(&path, b"before").expect("initial bytes");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let error = DirectFileSystem
        .write_file_if_unchanged(
            &uri,
            b"before".to_vec(),
            b"after".to_vec(),
            WriteFileOptions::default(),
            /*sandbox*/ None,
        )
        .await
        .expect_err("unsupported default");
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert_eq!(std::fs::read(path).expect("unchanged bytes"), b"before");
}

#[tokio::test]
async fn cancellation_before_mutation_admission_starts_no_effect() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical temporary root");
    let path = root.join("file");
    std::fs::write(&path, b"before").expect("initial bytes");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let mut pause = Pause::new(&path, Phase::Blocking);
    let first_uri = uri.clone();
    let first = tokio::spawn(async move {
        filesystem()
            .write_file(
                &first_uri,
                b"first".to_vec(),
                WriteFileOptions {
                    follow_symlinks: false,
                },
                /*sandbox*/ None,
            )
            .await
    });
    pause.entered().await;
    let file_system = filesystem();
    let mut queued = file_system.write_file_if_unchanged(
        &uri,
        b"first".to_vec(),
        b"cancelled".to_vec(),
        WriteFileOptions::default(),
        /*sandbox*/ None,
    );
    assert!(futures::poll!(queued.as_mut()).is_pending());
    drop(queued);
    pause.release();
    first.await.expect("first task").expect("first write");
    assert_eq!(std::fs::read(path).expect("final bytes"), b"first");
}

#[test_case("conditional"; "conditional")]
#[test_case("write"; "ordinary_write")]
#[test_case("remove"; "ordinary_remove")]
#[test_case("copy"; "ordinary_copy")]
#[test_case("recursive_copy"; "recursive_later_target")]
#[tokio::test]
async fn cancelled_waiter_retains_its_real_blocking_effect(kind: &str) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical temporary root");
    let destination = root.join("destination");
    let source = root.join("source");
    std::fs::create_dir_all(destination.join("nested")).expect("destination");
    std::fs::create_dir_all(source.join("nested")).expect("source");
    let path = destination.join("nested/file");
    let source_file = source.join("nested/file");
    std::fs::write(&path, b"before").expect("initial bytes");
    std::fs::write(&source_file, b"first").expect("source bytes");
    let uri = PathUri::from_host_native_path(&path).expect("path URI");
    let mut pause = Pause::new(&path, Phase::Blocking);
    let first_uri = uri.clone();
    let operation = kind.to_string();
    let first = tokio::spawn(async move {
        let fs = filesystem();
        match operation.as_str() {
            "conditional" => fs
                .write_file_if_unchanged(
                    &first_uri,
                    b"before".to_vec(),
                    b"first".to_vec(),
                    WriteFileOptions {
                        follow_symlinks: false,
                    },
                    /*sandbox*/ None,
                )
                .await
                .map(|_| ()),
            "write" => {
                fs.write_file(
                    &first_uri,
                    b"first".to_vec(),
                    WriteFileOptions {
                        follow_symlinks: false,
                    },
                    /*sandbox*/ None,
                )
                .await
            }
            "remove" => {
                fs.remove(
                    &first_uri,
                    RemoveOptions {
                        recursive: false,
                        force: false,
                        follow_symlinks: false,
                    },
                    /*sandbox*/ None,
                )
                .await
            }
            "copy" => {
                fs.copy(
                    &PathUri::from_host_native_path(source_file).expect("source URI"),
                    &first_uri,
                    CopyOptions { recursive: false },
                    /*sandbox*/ None,
                )
                .await
            }
            "recursive_copy" => {
                fs.copy(
                    &PathUri::from_host_native_path(source).expect("source URI"),
                    &PathUri::from_host_native_path(destination).expect("destination URI"),
                    CopyOptions { recursive: true },
                    /*sandbox*/ None,
                )
                .await
            }
            other => panic!("unexpected operation {other}"),
        }
    });
    pause.entered().await;
    first.abort();
    assert!(first.await.expect_err("cancelled caller").is_cancelled());
    // No competing owner has started: this permit still belongs to the surviving effect.
    assert_eq!(available_permits(), 0);
    let fs = filesystem();
    let mut next = fs.write_file_if_unchanged(
        &uri,
        b"first".to_vec(),
        b"second".to_vec(),
        WriteFileOptions::default(),
        /*sandbox*/ None,
    );
    assert!(futures::poll!(next.as_mut()).is_pending());
    pause.release();
    let result = next.await;
    if kind == "remove" {
        assert_eq!(
            result.expect_err("removed before comparison").kind(),
            io::ErrorKind::NotFound
        );
        assert!(!path.exists());
    } else {
        assert_eq!(
            result.expect("successor write"),
            ConditionalWriteResult::Written
        );
        assert_eq!(std::fs::read(path).expect("ordered bytes"), b"second");
    }
}

fn filesystem() -> Arc<dyn ExecutorFileSystem> {
    Arc::new(LocalFileSystem::unsandboxed())
}
