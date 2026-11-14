//! Workflows: data, not code.
//!
//! * [`definition`] – `WorkflowDefinition` with steps, transitions, variables, triggers and AI policy.
//! * [`input`] – module-owned `InputSchema`s; forms are generated from them; values are validated.
//! * [`expr`] – the safe, side-effect-free expression language (filters, conditions, parameters).
//! * [`analysis`] – definition validation and dependency extraction.
//! * [`impact`] – classification of a workflow against a module's contract-surface changes (+ auto-migration).
//! * [`instance`] – running instances, history, pending tasks, AI proposals.
//! * [`api`] – request/response types of the workflow and AI services.

pub mod analysis;
pub mod api;
pub mod definition;
pub mod expr;
pub mod impact;
pub mod input;
pub mod instance;

pub use definition::*;
pub use instance::*;
