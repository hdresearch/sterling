//! Base image creation and configuration module.
//!
//! This module provides functionality for creating and configuring base images
//! that can be used to spawn VMs. It supports creating images from Docker images
//! or S3 tarballs, and automatically configures the filesystem with Chelsea's
//! required scripts (networking, ready notification, etc.).

mod builder;
mod config;
mod error;

pub use builder::{
    BaseImageBuilder, CreateBaseImageRequest, ImageCreationStatus, ImageSource, base_image_exists,
    delete_base_image, list_base_images,
};
pub use config::configure_filesystem;
pub use error::BaseImageError;
