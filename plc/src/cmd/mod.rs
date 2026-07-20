//! Subcommand implementations. Each `run` resolves/creates a file and returns
//! its path; `main` prints it. Subcommands are added one per PR.

pub mod calview;
pub mod daily;
pub mod do_notes;
pub mod doctor;
pub mod ledger;
pub mod init;
pub mod isg;
pub mod murmur;
pub mod orphans;
pub mod shot;
pub mod stat;
pub mod top;
pub mod weekly;
