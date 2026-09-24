//! File operations on real folders: what a target matches, copying a save
//! set into a checkpoint, applying a checkpoint in four reversible stages,
//! change signatures and deletion by rename.

pub mod checkpoint;
pub mod fsx;
pub mod load;
pub mod walk;

pub use checkpoint::{
    CheckpointMeta, DISPOSAL_PREFIX, DisposeError, Hook, META_FILE, Signature, TEMP_PREFIX, copy_save_set, dispose,
    folder_size, is_reserved_folder, no_hook, publish, read_meta, remove_disposal, signature,
};
pub use fsx::{identity, presence, real_path};
pub use load::{LoadFile, LoadPlan, StageError, plan_load};
pub use walk::{Entry, walk_target};
