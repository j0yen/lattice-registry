mod catalog;
mod commands;
mod error;
mod fetcher;
mod owl_analysis;
mod registry;

use clap::{Parser, Subcommand};
use std::process;

use catalog::{default_cache_dir, default_catalog_path};
use error::Error;

#[derive(Parser)]
#[command(
    name = "lattice-registry",
    about = "Local catalog of BFO-grounded ontologies for the ethical lattice",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ingest OBO Foundry (or other) registry and populate local catalog
    Sync {
        /// Registry to ingest (default: obo-foundry)
        #[arg(long, default_value = "obo-foundry")]
        registry: String,
    },
    /// Fetch + catalog a single ontology by IRI or URL
    Add {
        /// IRI or URL of the OWL/OBO file to fetch
        iri: String,
    },
    /// List ontologies in the local catalog
    List {
        /// Filter by domain
        #[arg(long)]
        domain: Option<String>,
    },
    /// Show full metadata for one catalog entry
    Show {
        /// Ontology ID
        id: String,
    },
    /// Print local cached .owl path for a catalog entry
    Path {
        /// Ontology ID
        id: String,
    },
}

fn main() {
    // Reset SIGPIPE so that piping to `head` etc. doesn't panic.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();

    let catalog_path = match default_catalog_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };
    let cache_dir = match default_cache_dir() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };

    let result = match &cli.command {
        Cmd::Sync { registry } => {
            commands::cmd_sync(&catalog_path, &cache_dir, registry)
        }
        Cmd::Add { iri } => {
            commands::cmd_add(&catalog_path, &cache_dir, iri)
        }
        Cmd::List { domain } => {
            commands::cmd_list(&catalog_path, domain.as_deref())
        }
        Cmd::Show { id } => {
            commands::cmd_show(&catalog_path, id)
        }
        Cmd::Path { id } => {
            commands::cmd_path(&catalog_path, id)
        }
    };

    match result {
        Ok(()) => {}
        Err(Error::NotFound(msg)) => {
            eprintln!("Not found: {}", msg);
            process::exit(2);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
