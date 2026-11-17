#![no_std]

pub mod config;
pub mod dhcp;
pub mod dns;
pub mod http;
pub mod mdns;
pub mod reminders;
pub mod sntp;
pub mod tz;

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
