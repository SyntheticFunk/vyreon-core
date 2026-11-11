//! vyreon-core's database: the migrations and (later) the queries over them. Core owns every
//! table and is the only writer (docs/DECISIONS.md C-019).
pub mod migrate;
pub mod universe;
