//! Orchestrator for manipulating nodes and recording results during a run of `HotShot` tests

/// The orchestrator's clients
pub mod client;
/// Configuration for the orchestrator
pub mod config;

use async_lock::RwLock;
use hotshot_types::traits::{election::ElectionConfig, signature_key::SignatureKey};
use std::{
    collections::HashSet,
    io,
    io::ErrorKind,
    net::{IpAddr, SocketAddr},
};
use tide_disco::{Api, App};

use surf_disco::Url;
use tide_disco::{
    api::ApiError,
    error::ServerError,
    method::{ReadState, WriteState},
};

use futures::FutureExt;

use crate::config::NetworkConfig;

use libp2p::identity::{
    ed25519::{Keypair as EdKeypair, SecretKey},
    Keypair,
};
/// Generate an keypair based on a `seed` and an `index`
/// # Panics
/// This panics if libp2p is unable to generate a secret key from the seed
#[must_use]
pub fn libp2p_generate_indexed_identity(seed: [u8; 32], index: u64) -> Keypair {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&seed);
    hasher.update(&index.to_le_bytes());
    let new_seed = *hasher.finalize().as_bytes();
    let sk_bytes = SecretKey::try_from_bytes(new_seed).unwrap();
    <EdKeypair as From<SecretKey>>::from(sk_bytes).into()
}

/// The state of the orchestrator
#[derive(Default, Clone)]
struct OrchestratorState<KEY: SignatureKey, ELECTION: ElectionConfig> {
    /// Tracks the latest node index we have generated a configuration for
    latest_index: u16,
    /// The network configuration
    config: NetworkConfig<KEY, ELECTION>,
    /// The total nodes that have posted their public keys
    pub nodes_with_pubkey: u64,
    /// Whether the network configuration has been updated with all the peer's public keys/configs
    peer_pub_ready: bool,
    /// The set of index for nodes that have posted their public keys/configs
    pub_posted: HashSet<u64>,
    /// Whether nodes should start their HotShot instances
    /// Will be set to true once all nodes post they are ready to start
    start: bool,
    /// The total nodes that have posted they are ready to start
    pub nodes_connected: u64,
}

impl<KEY: SignatureKey + 'static, ELECTION: ElectionConfig + 'static>
    OrchestratorState<KEY, ELECTION>
{
    /// create a new [`OrchestratorState`]
    pub fn new(network_config: NetworkConfig<KEY, ELECTION>) -> Self {
        OrchestratorState {
            latest_index: 0,
            config: network_config,
            nodes_with_pubkey: 0,
            peer_pub_ready: false,
            pub_posted: HashSet::new(),
            nodes_connected: 0,
            start: false,
        }
    }
}

/// An api exposed by the orchestrator
pub trait OrchestratorApi<KEY: SignatureKey, ELECTION: ElectionConfig> {
    /// post endpoint for identity
    /// # Errors
    /// if unable to serve
    fn post_identity(&mut self, identity: IpAddr) -> Result<u16, ServerError>;
    /// post endpoint for each node's config
    /// # Errors
    /// if unable to serve
    fn post_getconfig(
        &mut self,
        _node_index: u16,
    ) -> Result<NetworkConfig<KEY, ELECTION>, ServerError>;
    /// post endpoint for each node's public key
    /// # Errors
    /// if unable to serve
    fn register_public_key(
        &mut self,
        node_index: u64,
        pubkey: &mut Vec<u8>,
    ) -> Result<(), ServerError>;
    /// post endpoint for whether or not all peers public keys are ready
    /// # Errors
    /// if unable to serve
    fn peer_pub_ready(&self) -> Result<bool, ServerError>;
    /// get endpoint for the network config after all peers public keys are collected
    /// # Errors
    /// if unable to serve
    fn get_config_after_peer_collected(&self) -> Result<NetworkConfig<KEY, ELECTION>, ServerError>;
    /// get endpoint for whether or not the run has started
    /// # Errors
    /// if unable to serve
    fn get_start(&self) -> Result<bool, ServerError>;
    /// post endpoint for whether or not all nodes are ready
    /// # Errors
    /// if unable to serve
    fn post_ready(&mut self) -> Result<(), ServerError>;
    /// post endpoint for the results of the run
    /// # Errors
    /// if unable to serve
    fn post_run_results(&mut self) -> Result<(), ServerError>;
}

impl<KEY, ELECTION> OrchestratorApi<KEY, ELECTION> for OrchestratorState<KEY, ELECTION>
where
    KEY: serde::Serialize + Clone + SignatureKey,
    ELECTION: serde::Serialize + Clone + Send + ElectionConfig,
{
    fn post_identity(&mut self, identity: IpAddr) -> Result<u16, ServerError> {
        let node_index = self.latest_index;
        self.latest_index += 1;

        // TODO https://github.com/EspressoSystems/HotShot/issues/850
        if usize::from(node_index) >= self.config.config.total_nodes.get() {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Network has reached capacity".to_string(),
            });
        }

        if self.config.libp2p_config.clone().is_some() {
            let libp2p_config_clone = self.config.libp2p_config.clone().unwrap();
            // Designate node as bootstrap node and store its identity information
            if libp2p_config_clone.bootstrap_nodes.len() < libp2p_config_clone.num_bootstrap_nodes {
                let port_index = if libp2p_config_clone.index_ports {
                    node_index
                } else {
                    0
                };
                let socketaddr =
                    SocketAddr::new(identity, libp2p_config_clone.base_port + port_index);
                let keypair = libp2p_generate_indexed_identity(self.config.seed, node_index.into());
                self.config
                    .libp2p_config
                    .as_mut()
                    .unwrap()
                    .bootstrap_nodes
                    .push((socketaddr, keypair.to_protobuf_encoding().unwrap()));
            }
        }
        Ok(node_index)
    }

    // Assumes nodes will set their own index that they received from the
    // 'identity' endpoint
    fn post_getconfig(
        &mut self,
        _node_index: u16,
    ) -> Result<NetworkConfig<KEY, ELECTION>, ServerError> {
        if self.config.libp2p_config.is_some() {
            let libp2p_config = self.config.clone().libp2p_config.unwrap();
            if libp2p_config.bootstrap_nodes.len() < libp2p_config.num_bootstrap_nodes {
                return Err(ServerError {
                    status: tide_disco::StatusCode::BadRequest,
                    message: "Not enough bootstrap nodes have registered".to_string(),
                });
            }
        }
        Ok(self.config.clone())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn register_public_key(
        &mut self,
        node_index: u64,
        pubkey: &mut Vec<u8>,
    ) -> Result<(), ServerError> {
        if self.pub_posted.contains(&node_index) {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Node has already posted public key".to_string(),
            });
        }
        self.pub_posted.insert(node_index);

        // Sishan NOTE: let me know if there's a better way to remove the first extra 8 bytes
        // The guess is extra bytes are from orchestrator serialization
        pubkey.drain(..8);
        let register_pub_key = <KEY as SignatureKey>::from_bytes(pubkey).unwrap();
        let register_pub_key_with_stake = register_pub_key.get_stake_table_entry(1u64);
        self.config.config.known_nodes_with_stake[node_index as usize] =
            register_pub_key_with_stake;
        self.nodes_with_pubkey += 1;
        println!(
            "Node {:?} posted public key, now total num posted public key: {:?}",
            node_index, self.nodes_with_pubkey
        );
        if self.nodes_with_pubkey >= (self.config.config.total_nodes.get() as u64) {
            self.peer_pub_ready = true;
        }
        Ok(())
    }

    fn peer_pub_ready(&self) -> Result<bool, ServerError> {
        if !self.peer_pub_ready {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Peer's public configs are not ready".to_string(),
            });
        }
        Ok(self.peer_pub_ready)
    }

    fn get_config_after_peer_collected(&self) -> Result<NetworkConfig<KEY, ELECTION>, ServerError> {
        if !self.peer_pub_ready {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Peer's public configs are not ready".to_string(),
            });
        }
        Ok(self.config.clone())
    }

    fn get_start(&self) -> Result<bool, ServerError> {
        // println!("{}", self.start);
        if !self.start {
            return Err(ServerError {
                status: tide_disco::StatusCode::BadRequest,
                message: "Network is not ready to start".to_string(),
            });
        }
        Ok(self.start)
    }

    // Assumes nodes do not post 'ready' twice
    // TODO ED Add a map to verify which nodes have posted they're ready
    fn post_ready(&mut self) -> Result<(), ServerError> {
        self.nodes_connected += 1;
        println!("Nodes connected: {}", self.nodes_connected);
        if self.nodes_connected >= (self.config.config.total_nodes.get() as u64) {
            self.start = true;
        }
        Ok(())
    }

    fn post_run_results(&mut self) -> Result<(), ServerError> {
        Ok(())
    }
}

/// Sets up all API routes
fn define_api<KEY: SignatureKey, ELECTION: ElectionConfig, State>(
) -> Result<Api<State, ServerError>, ApiError>
where
    State: 'static + Send + Sync + ReadState + WriteState,
    <State as ReadState>::State: Send + Sync + OrchestratorApi<KEY, ELECTION>,
    KEY: serde::Serialize,
    ELECTION: serde::Serialize,
{
    let api_toml = toml::from_str::<toml::Value>(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/api.toml"
    )))
    .expect("API file is not valid toml");
    let mut api = Api::<State, ServerError>::new(api_toml)?;
    api.post("postidentity", |req, state| {
        async move {
            let identity = req.string_param("identity")?.parse::<IpAddr>();
            if identity.is_err() {
                return Err(ServerError {
                    status: tide_disco::StatusCode::BadRequest,
                    message: "Identity is not a properly formed IP address".to_string(),
                });
            }
            state.post_identity(identity.unwrap())
        }
        .boxed()
    })?
    .post("post_getconfig", |req, state| {
        async move {
            let node_index = req.integer_param("node_index")?;
            state.post_getconfig(node_index)
        }
        .boxed()
    })?
    .post("postpubkey", |req, state| {
        async move {
            let node_index = req.integer_param("node_index")?;
            let mut pubkey = req.body_bytes();
            state.register_public_key(node_index, &mut pubkey)
        }
        .boxed()
    })?
    .get("peer_pubconfig_ready", |_req, state| {
        async move { state.peer_pub_ready() }.boxed()
    })?
    .get("config_after_peer_collected", |_req, state| {
        async move { state.get_config_after_peer_collected() }.boxed()
    })?
    .post(
        "postready",
        |_req, state: &mut <State as ReadState>::State| async move { state.post_ready() }.boxed(),
    )?
    .get("getstart", |_req, state| {
        async move { state.get_start() }.boxed()
    })?
    .post("postresults", |_req, state| {
        async move { state.post_run_results() }.boxed()
    })?;
    Ok(api)
}

/// Runs the orchestrator
/// # Errors
/// This errors if tide disco runs into an issue during serving
/// # Panics
/// This panics if unable to register the api with tide disco
pub async fn run_orchestrator<KEY, ELECTION>(
    network_config: NetworkConfig<KEY, ELECTION>,
    url: Url,
) -> io::Result<()>
where
    KEY: SignatureKey + 'static + serde::Serialize,
    ELECTION: ElectionConfig + 'static + serde::Serialize,
{
    let web_api =
        define_api().map_err(|_e| io::Error::new(ErrorKind::Other, "Failed to define api"));

    let state: RwLock<OrchestratorState<KEY, ELECTION>> =
        RwLock::new(OrchestratorState::new(network_config));

    let mut app = App::<RwLock<OrchestratorState<KEY, ELECTION>>, ServerError>::with_state(state);
    app.register_module("api", web_api.unwrap())
        .expect("Error registering api");
    tracing::error!("listening on {:?}", url);
    app.serve(url).await
}
