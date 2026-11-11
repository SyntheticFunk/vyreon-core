//! The migrations are embedded by a macro that cargo cannot see through: without this, adding or
//! editing a migration file would not rebuild the crate.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
