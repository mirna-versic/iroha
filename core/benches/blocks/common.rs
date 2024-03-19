use std::str::FromStr as _;

use iroha_core::{
    block::{BlockBuilder, CommittedBlock},
    prelude::*,
    query::store::LiveQueryStore,
    smartcontracts::{Execute, Registrable as _},
    sumeragi::network_topology::Topology,
    wsv::World,
};
use iroha_data_model::{
    account::Account,
    asset::{AssetDefinition, AssetDefinitionId},
    domain::Domain,
    isi::InstructionBox,
    prelude::*,
    transaction::TransactionLimits,
    ChainId,
};
use iroha_primitives::unique_vec::UniqueVec;
use serde_json::json;

/// Create block
pub fn create_block(
    wsv: &mut WorldStateView,
    instructions: Vec<InstructionBox>,
    account_id: AccountId,
    key_pair: &KeyPair,
) -> CommittedBlock {
    let chain_id = ChainId::from("0");

    let transaction = TransactionBuilder::new(chain_id.clone(), account_id)
        .with_instructions(instructions)
        .sign(key_pair);
    let limits = wsv.transaction_executor().transaction_limits;

    let topology = Topology::new(UniqueVec::new());
    let block = BlockBuilder::new(
        vec![AcceptedTransaction::accept(transaction, &chain_id, &limits).unwrap()],
        topology.clone(),
        Vec::new(),
    )
    .chain(0, wsv)
    .sign(key_pair)
    .commit(&topology)
    .unwrap();

    // Verify that transactions are valid
    for tx in block.as_ref().transactions() {
        assert_eq!(tx.error, None);
    }

    block
}

pub fn populate_wsv(
    domains: usize,
    accounts_per_domain: usize,
    assets_per_domain: usize,
    owner_id: &AccountId,
) -> Vec<InstructionBox> {
    let mut instructions: Vec<InstructionBox> = Vec::new();
    for i in 0..domains {
        let domain_id = construct_domain_id(i);
        let domain = Domain::new(domain_id.clone());
        instructions.push(Register::domain(domain).into());
        let can_unregister_domain = Mint::permission(
            PermissionToken::new(
                "CanUnregisterDomain".parse().unwrap(),
                &json!({ "domain_id": domain_id.clone() }),
            ),
            owner_id.clone(),
        );
        instructions.push(can_unregister_domain.into());
        for j in 0..accounts_per_domain {
            let account_id = construct_account_id(j, domain_id.clone());
            let account = Account::new(account_id.clone(), KeyPair::random().into_parts().0);
            instructions.push(Register::account(account).into());
            let can_unregister_account = Mint::permission(
                PermissionToken::new(
                    "CanUnregisterAccount".parse().unwrap(),
                    &json!({ "account_id": account_id.clone() }),
                ),
                owner_id.clone(),
            );
            instructions.push(can_unregister_account.into());
        }
        for k in 0..assets_per_domain {
            let asset_definition_id = construct_asset_definition_id(k, domain_id.clone());
            let asset_definition = AssetDefinition::numeric(asset_definition_id.clone());
            instructions.push(Register::asset_definition(asset_definition).into());
            let can_unregister_asset_definition = Mint::permission(
                PermissionToken::new(
                    "CanUnregisterAssetDefinition".parse().unwrap(),
                    &json!({ "asset_definition_id": asset_definition_id }),
                ),
                owner_id.clone(),
            );
            instructions.push(can_unregister_asset_definition.into());
        }
    }
    instructions
}

pub fn delete_every_nth(
    domains: usize,
    accounts_per_domain: usize,
    assets_per_domain: usize,
    nth: usize,
) -> Vec<InstructionBox> {
    let mut instructions: Vec<InstructionBox> = Vec::new();
    for i in 0..domains {
        let domain_id = construct_domain_id(i);
        if i % nth == 0 {
            instructions.push(Unregister::domain(domain_id.clone()).into());
        } else {
            for j in 0..accounts_per_domain {
                if j % nth == 0 {
                    let account_id = construct_account_id(j, domain_id.clone());
                    instructions.push(Unregister::account(account_id.clone()).into());
                }
            }
            for k in 0..assets_per_domain {
                if k % nth == 0 {
                    let asset_definition_id = construct_asset_definition_id(k, domain_id.clone());
                    instructions.push(Unregister::asset_definition(asset_definition_id).into());
                }
            }
        }
    }
    instructions
}

pub fn restore_every_nth(
    domains: usize,
    accounts_per_domain: usize,
    assets_per_domain: usize,
    nth: usize,
) -> Vec<InstructionBox> {
    let mut instructions: Vec<InstructionBox> = Vec::new();
    for i in 0..domains {
        let domain_id = construct_domain_id(i);
        if i % nth == 0 {
            let domain = Domain::new(domain_id.clone());
            instructions.push(Register::domain(domain).into());
        }
        for j in 0..accounts_per_domain {
            if j % nth == 0 || i % nth == 0 {
                let account_id = construct_account_id(j, domain_id.clone());
                let account = Account::new(account_id.clone(), KeyPair::random().into_parts().0);
                instructions.push(Register::account(account).into());
            }
        }
        for k in 0..assets_per_domain {
            if k % nth == 0 || i % nth == 0 {
                let asset_definition_id = construct_asset_definition_id(k, domain_id.clone());
                let asset_definition = AssetDefinition::numeric(asset_definition_id);
                instructions.push(Register::asset_definition(asset_definition).into());
            }
        }
    }
    instructions
}

pub fn build_wsv(
    rt: &tokio::runtime::Handle,
    account_id: &AccountId,
    key_pair: &KeyPair,
) -> WorldStateView {
    let kura = iroha_core::kura::Kura::blank_kura_for_testing();
    let query_handle = {
        let _guard = rt.enter();
        LiveQueryStore::test().start()
    };
    let mut domain = Domain::new(account_id.domain_id.clone()).build(account_id);
    domain.accounts.insert(
        account_id.clone(),
        Account::new(account_id.clone(), key_pair.public_key().clone()).build(account_id),
    );
    let mut wsv = WorldStateView::new(World::with([domain], UniqueVec::new()), kura, query_handle);
    wsv.config.transaction_limits = TransactionLimits::new(u64::MAX, u64::MAX);
    wsv.config.executor_runtime.fuel_limit = u64::MAX;
    wsv.config.executor_runtime.max_memory_bytes = u32::MAX;
    wsv.config.wasm_runtime.fuel_limit = u64::MAX;
    wsv.config.wasm_runtime.max_memory_bytes = u32::MAX;

    {
        let path_to_executor = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../configs/swarm/executor.wasm");
        let wasm = std::fs::read(&path_to_executor)
            .unwrap_or_else(|_| panic!("Failed to read file: {}", path_to_executor.display()));
        let executor = Executor::new(WasmSmartContract::from_compiled(wasm));
        Upgrade::new(executor)
            .execute(account_id, &mut wsv)
            .expect("Failed to load executor");
    }

    wsv
}

fn construct_domain_id(i: usize) -> DomainId {
    DomainId::from_str(&format!("non_inlinable_domain_name_{i}")).unwrap()
}

fn construct_account_id(i: usize, domain_id: DomainId) -> AccountId {
    AccountId::new(
        domain_id,
        Name::from_str(&format!("non_inlinable_account_name_{i}")).unwrap(),
    )
}

fn construct_asset_definition_id(i: usize, domain_id: DomainId) -> AssetDefinitionId {
    AssetDefinitionId::new(
        domain_id,
        Name::from_str(&format!("non_inlinable_asset_definition_name_{i}")).unwrap(),
    )
}
