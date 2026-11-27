//! Credential storage backed by the Windows Credential Manager (REQ-WINAPP-075).
//!
//! - `pairing`: the Point Guard pairing secret (`Services/PiPairingSecret.swift`), service
//!   `PurePoint.PointGuard.Pairing` with the endpoint as account (WIN-9.7).

pub mod pairing;
