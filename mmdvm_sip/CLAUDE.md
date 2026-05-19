This rust project will be a DMR Gateway experiment. First it will be a gateway to SIP, then maybe a 
fully decentralized p2p network using some suitable crate.


For now use clap, thiserror, anyhow, zerocopy (for network packets), nom for advanvec parsing, toml for config files with serde.

I prefer to use subcommands in a command module and main.rs is a simple dispatcher. 

Prefer to put structs in a file with a snake_case name of the struct.

prefer to use straits and structs and impl rather then a lot of free functions.
