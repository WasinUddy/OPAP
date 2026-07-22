// Copyright (C) 2026 OPAP contributors
// SPDX-License-Identifier: GPL-3.0-only

//! Framework-neutral application services for OPAP.
//!
//! UI and native-host adapters should depend on this crate rather than calling
//! the importer and storage repositories directly. All boundary types are
//! serializable and all expected failures carry stable machine-readable codes.

mod api;
mod error;
mod service;

pub use api::*;
pub use error::{ApiError, ApiErrorCode, ApiResult};
pub use service::{AppService, Clock, SystemClock};
