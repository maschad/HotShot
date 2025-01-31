use hotshot::traits::election::static_committee::GeneralStaticCommittee;

use crate::{
    block_types::{TestBlockHeader, TestBlockPayload, TestTransaction},
    state_types::{TestInstanceState, TestValidatedState},
};

use hotshot::traits::{
    election::static_committee::{StaticCommittee, StaticElectionConfig},
    implementations::{
        CombinedNetworks, Libp2pNetwork, MemoryNetwork, MemoryStorage, WebServerNetwork,
    },
    NodeImplementation,
};
use hotshot_types::{
    data::ViewNumber, message::Message, signature_key::BLSPubKey,
    traits::node_implementation::NodeType,
};
use serde::{Deserialize, Serialize};

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
/// filler struct to implement node type and allow us
/// to select our traits
pub struct TestTypes;
impl NodeType for TestTypes {
    type Time = ViewNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership = GeneralStaticCommittee<TestTypes, Self::SignatureKey>;
}

/// Memory network implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct MemoryImpl;

/// Libp2p network implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct Libp2pImpl;

/// Web server network implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct WebImpl;

/// Combined Network implementation (libp2p + web sever)
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct CombinedImpl;

/// static committee type alias
pub type StaticMembership = StaticCommittee<TestTypes>;

/// memory network
pub type StaticMemoryDAComm =
    MemoryNetwork<Message<TestTypes>, <TestTypes as NodeType>::SignatureKey>;

/// libp2p network
type StaticLibp2pDAComm = Libp2pNetwork<Message<TestTypes>, <TestTypes as NodeType>::SignatureKey>;

/// web server network communication channel
type StaticWebDAComm = WebServerNetwork<TestTypes>;

/// combined network
type StaticCombinedDAComm = CombinedNetworks<TestTypes>;

/// memory comm channel
pub type StaticMemoryQuorumComm =
    MemoryNetwork<Message<TestTypes>, <TestTypes as NodeType>::SignatureKey>;

/// libp2p comm channel
type StaticLibp2pQuorumComm =
    Libp2pNetwork<Message<TestTypes>, <TestTypes as NodeType>::SignatureKey>;

/// web server comm channel
type StaticWebQuorumComm = WebServerNetwork<TestTypes>;

/// combined network (libp2p + web server)
type StaticCombinedQuorumComm = CombinedNetworks<TestTypes>;

impl NodeImplementation<TestTypes> for Libp2pImpl {
    type Storage = MemoryStorage<TestTypes>;
    type QuorumNetwork = StaticLibp2pQuorumComm;
    type CommitteeNetwork = StaticLibp2pDAComm;
}

impl NodeImplementation<TestTypes> for MemoryImpl {
    type Storage = MemoryStorage<TestTypes>;
    type QuorumNetwork = StaticMemoryQuorumComm;
    type CommitteeNetwork = StaticMemoryDAComm;
}

impl NodeImplementation<TestTypes> for WebImpl {
    type Storage = MemoryStorage<TestTypes>;
    type QuorumNetwork = StaticWebQuorumComm;
    type CommitteeNetwork = StaticWebDAComm;
}

impl NodeImplementation<TestTypes> for CombinedImpl {
    type Storage = MemoryStorage<TestTypes>;
    type QuorumNetwork = StaticCombinedQuorumComm;
    type CommitteeNetwork = StaticCombinedDAComm;
}
