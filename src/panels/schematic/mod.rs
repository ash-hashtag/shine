//! Circuit netlists: blocks connected port to port by waveguides.
//!
//! The editor draws everything on one grid (see [`crate::panels::optics`]);
//! this module keeps the block catalogue ([`model::Builtin`]), the netlist
//! used by the MCP server and older project files, and its simulator.

pub mod geometry;
pub mod model;
pub mod sim;
