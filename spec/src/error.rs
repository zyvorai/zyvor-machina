// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpecError {
    #[error("invalid name: {0}")]
    InvalidName(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("parse memory: {0}")]
    Memory(String),
}
