#[macro_use]
extern crate prost_derive;
#[macro_use]
extern crate serde_derive;

pub mod transit_realtime {
    include!(concat!(env!("OUT_DIR"), "/transit_realtime.rs"));
}
#[macro_use]
pub mod utils;

pub mod actors;
pub mod datasets;
pub mod middlewares;
pub(crate) mod model_update;
pub(crate) mod routes;
pub mod server;
pub mod siri_lite;

#[cfg(test)]
mod tests;
