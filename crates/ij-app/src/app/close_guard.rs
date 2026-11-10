use super::*;

impl ImageJApp {
    pub fn close_window(&mut self, id: ImageId) -> bool {
        if self.is_plugin_filter_running_for(id) {
            self.status_message =
                "Cannot close window while Java PlugInFilter is running; cancel it first".into();
            return false;
        }
        if self
            .pending_stack_process
            .as_ref()
            .is_some_and(|pending| pending.image_id == id)
        {
            self.pending_stack_process = None;
        }
        self.image_views.remove(&id);
        if self.dialogs.plugin_filter_target_id == Some(id) {
            self.dialogs.plugin_filter_target_id = None;
        }
        self.texture_cache.remove(&id);
        self.session.close_image(id)
    }

    pub fn close_all_windows(&mut self) {
        if self.is_plugin_filter_running() {
            self.status_message =
                "Cannot close all windows while Java PlugInFilter is running; cancel it first"
                    .into();
            return;
        }
        self.pending_stack_process = None;
        self.image_views.clear();
        self.dialogs.plugin_filter_target_id = None;
        self.texture_cache.clear();
        self.session.close_all_images();
        self.status_message = "All windows closed".to_string();
    }

    pub fn texture_upload_count(&self) -> u64 {
        self.texture_upload_count
    }

    pub fn texture_cache_hit_count(&self) -> u64 {
        self.texture_cache_hit_count
    }

    pub fn is_texture_cached(&self, id: ImageId) -> bool {
        self.texture_cache.contains_key(&id)
    }

    pub fn cached_texture_revision(&self, id: ImageId) -> Option<u64> {
        self.texture_cache.get(&id).map(|(rev, _)| *rev)
    }

    pub fn cached_texture_id(&self, id: ImageId) -> Option<egui::TextureId> {
        self.texture_cache.get(&id).map(|(_, handle)| handle.id())
    }

    pub fn reset_texture_counters(&mut self) {
        self.texture_upload_count = 0;
        self.texture_cache_hit_count = 0;
    }

    pub fn prune_texture_cache(&mut self) {
        self.texture_cache
            .retain(|id, _| self.session.get_image(*id).is_some());
    }

    pub fn is_dirty(&self, id: ImageId) -> bool {
        self.session.is_dirty(id)
    }

    pub fn set_dirty(&mut self, id: ImageId, dirty: bool) {
        self.session.set_dirty(id, dirty);
    }

    pub fn any_dirty(&self) -> bool {
        self.session.any_dirty()
    }

    /// True while a modal close/quit confirmation is unresolved.
    pub fn close_prompt_active(&self) -> bool {
        self.pending_close.is_some()
    }

    /// Returns an error and updates the status bar when a document-changing
    /// action is attempted while the close/quit modal is open. The modal is the
    /// single source of truth for which documents are being resolved, so
    /// creating, opening or mutating documents underneath it would either lose
    /// the new work or invalidate the prompt's snapshot.
    pub(super) fn reject_while_closing(&mut self, action: &str) -> Option<SessionError> {
        if self.is_plugin_filter_running() {
            let reason = "Java PlugInFilter is currently executing";
            self.status_message = format!("{action} blocked: {reason}");
            return Some(SessionError::InvalidPluginFilterRequest(format!(
                "{action} is unavailable: {reason}"
            )));
        }
        if self.pending_close.is_none() && !self.quit_app {
            return None;
        }
        let reason = if self.pending_close.is_some() {
            "resolve close confirmation first"
        } else {
            "application is exiting"
        };
        self.status_message = format!("{action} blocked: {reason}");
        Some(SessionError::UnknownCommand(format!(
            "{action} is unavailable: {reason}"
        )))
    }

    /// Requests closing a single window.
    /// If dirty, enters pending close confirmation guard and returns false (retaining window).
    /// If not dirty, closes immediately and returns true.
    /// While any prompt is open this is a deterministic no-op: it never
    /// replaces or downgrades the active prompt.
    pub fn request_close_window(&mut self, id: ImageId) -> bool {
        if self.is_plugin_filter_running_for(id) {
            self.status_message =
                "Close request ignored: Java PlugInFilter is executing on this image".into();
            return false;
        }
        if self.pending_close.is_some() {
            self.status_message =
                "Close request ignored: close confirmation is already open".into();
            return false;
        }
        if !self.is_dirty(id) {
            self.close_window(id);
            true
        } else {
            self.pending_close = Some(PendingClose {
                target: CloseTarget::Single(id),
                queue: vec![id],
                current_prompt: Some(id),
            });
            false
        }
    }

    /// Requests closing all windows.
    /// Clean windows are retained until the sequence is resolved, so Cancel
    /// preserves the entire document set. Dirty windows are prompted in order.
    /// Returns true if all windows were closed immediately with no pending confirmation.
    pub fn request_close_all(&mut self) -> bool {
        if self.is_plugin_filter_running() {
            self.status_message =
                "Close All ignored: Java PlugInFilter is currently executing".into();
            return false;
        }
        if self.pending_close.is_some() {
            self.status_message = "Close All ignored: close confirmation is already open".into();
            return false;
        }
        let dirty_queue = self.dirty_image_ids();
        if dirty_queue.is_empty() {
            self.close_all_windows();
            true
        } else {
            let first = dirty_queue[0];
            self.pending_close = Some(PendingClose {
                target: CloseTarget::All,
                queue: dirty_queue,
                current_prompt: Some(first),
            });
            false
        }
    }

    /// Requests quitting the application.
    /// Clean windows are retained until the sequence is resolved, so Cancel
    /// preserves every document. Dirty windows are prompted in order.
    /// Returns true if app can quit immediately.
    pub fn request_quit(&mut self) -> bool {
        if self.is_plugin_filter_running() {
            self.status_message =
                "Quit request ignored: Java PlugInFilter is currently executing; cancel it first"
                    .into();
            return false;
        }
        if self.pending_close.is_some() {
            // A repeated quit (Cmd-Q again, dock quit, root close) while the
            // modal is open must not rescan or rebuild the queue.
            self.status_message = "Quit request ignored: close confirmation is already open".into();
            return false;
        }
        if matches!(
            self.pending_plugin_catalog_job,
            Some(PendingPluginCatalogJob::Inspect { .. })
        ) {
            self.cancel_plugin_catalog_job();
            self.status_message =
                "Quit deferred: waiting for Java plugin inspection to stop; retry when it finishes"
                    .into();
            return false;
        }
        self.cancel_plugin_catalog_job();
        let dirty_queue = self.dirty_image_ids();
        if dirty_queue.is_empty() {
            self.close_all_windows();
            self.quit_app = true;
            crate::macos_bridge::set_quit_authorized(true);
            crate::macos_bridge::set_dirty_state(false);
            true
        } else {
            let first = dirty_queue[0];
            self.pending_close = Some(PendingClose {
                target: CloseTarget::Quit,
                queue: dirty_queue,
                current_prompt: Some(first),
            });
            crate::macos_bridge::set_quit_authorized(false);
            crate::macos_bridge::set_dirty_state(true);
            false
        }
    }

    /// Deterministic list of dirty documents (session order).
    fn dirty_image_ids(&self) -> Vec<ImageId> {
        self.session
            .image_ids()
            .into_iter()
            .filter(|&id| self.is_dirty(id))
            .collect()
    }

    /// Advances a close sequence after the prompted document was resolved.
    ///
    /// For Close All/Quit the whole session is re-checked before closing:
    /// a document created or dirtied after the prompt started must receive its
    /// own prompt rather than being discarded by the final close. A Single
    /// window close never touches other documents.
    fn advance_close_sequence(&mut self, mut pending: PendingClose) -> bool {
        if matches!(pending.target, CloseTarget::Single(_)) {
            self.pending_close = None;
            return true;
        }

        let dirty_remaining = self.dirty_image_ids();
        pending.queue = dirty_remaining.clone();
        if let Some(&next) = dirty_remaining.first() {
            pending.current_prompt = Some(next);
            self.pending_close = Some(pending);
            return false;
        }

        self.pending_close = None;
        self.close_all_windows();
        if pending.target == CloseTarget::Quit {
            self.quit_app = true;
            crate::macos_bridge::set_quit_authorized(true);
            crate::macos_bridge::set_dirty_state(false);
        }
        true
    }

    /// Cancels any pending close sequence. All pending and prompted documents are retained.
    pub fn confirm_pending_close_cancel(&mut self) {
        self.pending_close = None;
        self.status_message = "Close canceled".to_string();
        crate::macos_bridge::set_quit_authorized(false);
    }

    /// Discards unsaved changes for the currently prompted document and advances queue.
    /// Returns true if the full close sequence has finished.
    pub fn confirm_pending_close_discard(&mut self) -> bool {
        let Some(mut pending) = self.pending_close.take() else {
            return true;
        };
        let Some(curr_id) = pending.current_prompt else {
            return true;
        };

        if self.session.get_image(curr_id).is_some() {
            self.close_window(curr_id);
        }
        pending.queue.retain(|&id| id != curr_id);

        self.advance_close_sequence(pending)
    }

    /// Saves the currently prompted document to the given path.
    /// If path is None (user cancelled picker) or save fails, document is RETAINED and close is aborted.
    /// Returns Ok(true) if close sequence completed, Ok(false) if more pending or cancelled, or Err on write failure.
    pub fn confirm_pending_close_save(
        &mut self,
        path: Option<PathBuf>,
    ) -> Result<bool, SessionError> {
        let Some(pending) = self.pending_close.clone() else {
            return Ok(true);
        };
        let Some(curr_id) = pending.current_prompt else {
            return Ok(true);
        };

        let Some(path) = path else {
            // User cancelled the file picker dialog: retain document and the
            // still-unresolved prompt so closing remains guarded.
            self.status_message = "Save cancelled - document retained".to_string();
            crate::macos_bridge::set_quit_authorized(false);
            return Ok(false);
        };

        if let Err(e) = self.session.save_image(curr_id, &path) {
            // Failed save: retain document and prompt, abort close, surface error!
            self.status_message = format!("Save failed: {e} - document retained");
            self.dialogs.last_error = Some(format!("Failed to save \"{}\": {e}", path.display()));
            crate::macos_bridge::set_quit_authorized(false);
            return Err(e);
        }

        self.pending_close = None;
        self.status_message = format!("Saved \"{}\"", path.display());
        self.close_window(curr_id);
        let mut pending = pending;
        pending.queue.retain(|&id| id != curr_id);

        Ok(self.advance_close_sequence(pending))
    }

    /// Resolves the current close prompt at the document's remembered TIFF
    /// path, matching `FileSaver.save()` (original_imagej/ij/io/FileSaver.java
    /// 42-69). Returns `None` when the image was never opened from or saved to
    /// a file, so the caller must show a Save As picker instead.
    pub fn save_pending_close_to_remembered_path(&mut self) -> Option<Result<bool, SessionError>> {
        let id = self.pending_close.as_ref()?.current_prompt?;
        let path = self.session.image_path(id)?.to_path_buf();
        Some(self.confirm_pending_close_save(Some(path)))
    }

}
