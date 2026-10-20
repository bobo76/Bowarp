//! Folder watching for the file explorer and the file viewer: the platform watcher, changed
//! paths handed to the owner a batch at a time, and the explorer's set of watched folders.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{self, UnboundedReceiver};
use gpui_kit::{Context, Task, Window};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Changes arriving within this time are applied together.
const CHANGE_BATCH: Duration = Duration::from_millis(100);

/// A view told about changes in the folders its [`FolderWatcher`] watches.
pub trait WatchesFolders: Sized + 'static {
    /// Names the view in log messages.
    const OWNER: &'static str;

    /// Whether events of this kind are of no interest to the view.
    fn ignores(kind: &EventKind) -> bool;

    /// The watched paths that changed, a batch at a time.
    fn on_folder_changes(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    );
}

/// The platform watcher of one view, each folder watched on its own (not recursively).
pub struct FolderWatcher {
    /// `None` when the platform watcher could not start: nothing is watched then.
    watcher: Option<RecommendedWatcher>,
    _changes: Task<()>,
}

impl FolderWatcher {
    pub fn new<V: WatchesFolders>(window: &Window, cx: &mut Context<V>) -> Self {
        let (sender, receiver) = mpsc::unbounded();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else {
                return;
            };
            if V::ignores(&event.kind) {
                return;
            }
            for path in event.paths {
                let _ = sender.unbounded_send(path);
            }
        })
        .inspect_err(|err| log::warn!("{}: no folder watcher: {err}", V::OWNER))
        .ok();
        Self {
            watcher,
            _changes: deliver_batches(receiver, window, cx),
        }
    }

    pub fn is_available(&self) -> bool {
        self.watcher.is_some()
    }

    /// Start watching `folder`. Fails when the platform watcher could not start.
    pub fn watch(&mut self, folder: &Path) -> notify::Result<()> {
        match self.watcher.as_mut() {
            Some(watcher) => watcher.watch(folder, RecursiveMode::NonRecursive),
            None => Err(notify::Error::generic("no folder watcher")),
        }
    }

    pub fn unwatch(&mut self, folder: &Path) {
        if let Some(watcher) = self.watcher.as_mut() {
            let _ = watcher.unwatch(folder);
        }
    }
}

/// Hand the changed paths to the view, a batch at a time.
fn deliver_batches<V: WatchesFolders>(
    mut changes: UnboundedReceiver<PathBuf>,
    window: &Window,
    cx: &mut Context<V>,
) -> Task<()> {
    cx.spawn_in(window, async move |this, cx| {
        while let Some(first) = changes.next().await {
            cx.background_executor().timer(CHANGE_BATCH).await;
            let mut paths = vec![first];
            while let Ok(path) = changes.try_recv() {
                paths.push(path);
            }
            if this
                .update_in(cx, |view, window, cx| {
                    view.on_folder_changes(paths, window, cx)
                })
                .is_err()
            {
                break;
            }
        }
    })
}

/// A set of folders kept watched, each watched once by its real path (what the watcher
/// reports): a folder and a symlink to it share one watch.
pub struct WatchedFolders {
    watcher: FolderWatcher,
    /// Each watched real path, with the folders showing it.
    watched: HashMap<PathBuf, Vec<PathBuf>>,
    /// The real path of each folder, so it is not looked up on every sync.
    real_paths: HashMap<PathBuf, PathBuf>,
}

impl WatchedFolders {
    pub fn new(watcher: FolderWatcher) -> Self {
        Self {
            watcher,
            watched: HashMap::new(),
            real_paths: HashMap::new(),
        }
    }

    /// Each watched real path, with the folders showing it.
    pub fn watched(&self) -> &HashMap<PathBuf, Vec<PathBuf>> {
        &self.watched
    }

    /// Forget the watched folders among `paths`: a watched folder reported about itself was
    /// removed or replaced, and the watcher drops its watch then, so the next sync watches it
    /// again.
    pub fn forget_reported(&mut self, paths: &[PathBuf]) {
        for path in paths {
            if self.watched.remove(path).is_some() {
                self.real_paths.retain(|_, real| real != path);
            }
        }
    }

    /// Watch exactly the folders in `open`.
    pub fn sync(&mut self, open: HashSet<PathBuf>) {
        if !self.watcher.is_available() {
            return;
        }
        self.real_paths.retain(|folder, _| open.contains(folder));
        let mut watched: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
        for folder in open {
            watched
                .entry(self.real_path_of(&folder))
                .or_default()
                .push(folder);
        }
        for real in self.watched.keys() {
            if !watched.contains_key(real) {
                self.watcher.unwatch(real);
            }
        }
        // Only folders whose watch holds are recorded, so a failed one is tried again.
        watched.retain(|real, _| {
            self.watched.contains_key(real)
                || self
                    .watcher
                    .watch(real)
                    .inspect_err(|err| {
                        log::debug!("file explorer: not watching {}: {err}", real.display())
                    })
                    .is_ok()
        });
        self.watched = watched;
    }

    fn real_path_of(&mut self, folder: &Path) -> PathBuf {
        if let Some(real) = self.real_paths.get(folder) {
            return real.clone();
        }
        // Not there (yet): look again next time.
        let Ok(real) = fs::canonicalize(folder) else {
            return folder.to_path_buf();
        };
        self.real_paths.insert(folder.to_path_buf(), real.clone());
        real
    }
}
