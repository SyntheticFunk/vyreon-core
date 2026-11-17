//! vyreon-core's database: the migrations and (later) the queries over them. Core owns every
//! table and is the only writer (docs/DECISIONS.md C-019).
pub mod api_key;
pub mod budget;
pub mod build;
pub mod capture;
pub mod daily;
pub mod evidence;
pub mod migrate;
pub mod notice;
pub mod publish;
pub mod tape;
pub mod universe;
pub mod watch;
pub mod work;
