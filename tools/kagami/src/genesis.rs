use std::path::PathBuf;

use clap::{Parser, Subcommand};
use iroha_config::parameters::defaults::chain_wide::{
    DEFAULT_BLOCK_TIME, DEFAULT_COMMIT_TIME, DEFAULT_IDENT_LENGTH_LIMITS, DEFAULT_MAX_TXS,
    DEFAULT_METADATA_LIMITS, DEFAULT_TRANSACTION_LIMITS, DEFAULT_WASM_FUEL_LIMIT,
    DEFAULT_WASM_MAX_MEMORY_BYTES,
};
use iroha_data_model::{
    metadata::Limits,
    parameter::{default::*, ParametersBuilder},
    prelude::AssetId,
};
use iroha_genesis::{executor_state, RawGenesisBlockBuilder, RawGenesisBlockFile};
use serde_json::json;

use super::*;

#[derive(Parser, Debug, Clone)]
pub struct Args {
    /// Specifies the `executor_file` <PATH> that will be inserted into the genesis JSON as-is.
    #[clap(long, value_name = "PATH")]
    executor_path_in_genesis: PathBuf,
    #[clap(subcommand)]
    mode: Option<Mode>,
}

#[derive(Subcommand, Debug, Clone, Default)]
pub enum Mode {
    /// Generate default genesis
    #[default]
    Default,
    /// Generate synthetic genesis with the specified number of domains, accounts and assets.
    ///
    /// Synthetic mode is useful when we need a semi-realistic genesis for stress-testing
    /// Iroha's startup times as well as being able to just start an Iroha network and have
    /// instructions that represent a typical blockchain after migration.
    Synthetic {
        /// Number of domains in synthetic genesis.
        #[clap(long, default_value_t)]
        domains: u64,
        /// Number of accounts per domains in synthetic genesis.
        /// The total number of accounts would be `domains * assets_per_domain`.
        #[clap(long, default_value_t)]
        accounts_per_domain: u64,
        /// Number of assets per domains in synthetic genesis.
        /// The total number of assets would be `domains * assets_per_domain`.
        #[clap(long, default_value_t)]
        assets_per_domain: u64,
    },
}

impl<T: Write> RunArgs<T> for Args {
    fn run(self, writer: &mut BufWriter<T>) -> Outcome {
        let Self {
            executor_path_in_genesis,
            mode,
        } = self;

        let builder = RawGenesisBlockBuilder::default().executor_file(executor_path_in_genesis);
        let genesis = match mode.unwrap_or_default() {
            Mode::Default => generate_default(builder),
            Mode::Synthetic {
                domains,
                accounts_per_domain,
                assets_per_domain,
            } => generate_synthetic(builder, domains, accounts_per_domain, assets_per_domain),
        }?;
        writeln!(writer, "{}", serde_json::to_string_pretty(&genesis)?)
            .wrap_err("failed to write serialized genesis to the buffer")
    }
}

#[allow(clippy::too_many_lines)]
pub fn generate_default(
    builder: RawGenesisBlockBuilder<executor_state::SetPath>,
) -> color_eyre::Result<RawGenesisBlockFile> {
    let mut meta = Metadata::new();
    meta.insert_with_limits("key".parse()?, "value".to_owned(), Limits::new(1024, 1024))?;

    let mut genesis = builder
        .domain_with_metadata("wonderland".parse()?, meta.clone())
        .account_with_metadata(
            "alice".parse()?,
            crate::DEFAULT_PUBLIC_KEY.parse()?,
            meta.clone(),
        )
        .account_with_metadata("bob".parse()?, crate::DEFAULT_PUBLIC_KEY.parse()?, meta)
        .asset(
            "rose".parse()?,
            AssetValueType::Numeric(NumericSpec::default()),
        )
        .finish_domain()
        .domain("garden_of_live_flowers".parse()?)
        .account("carpenter".parse()?, crate::DEFAULT_PUBLIC_KEY.parse()?)
        .asset(
            "cabbage".parse()?,
            AssetValueType::Numeric(NumericSpec::default()),
        )
        .finish_domain()
        .build();

    let alice_id = AccountId::from_str("alice@wonderland")?;
    let mint = Mint::asset_numeric(
        13u32,
        AssetId::new("rose#wonderland".parse()?, alice_id.clone()),
    );
    let mint_cabbage = Mint::asset_numeric(
        44u32,
        AssetId::new("cabbage#garden_of_live_flowers".parse()?, alice_id.clone()),
    );
    let grant_permission_to_set_parameters = Mint::permission(
        PermissionToken::new("CanSetParameters".parse()?, &json!(null)),
        alice_id.clone(),
    );
    let transfer_domain_ownerhip = Transfer::domain(
        "genesis@genesis".parse()?,
        "wonderland".parse()?,
        alice_id.clone(),
    );
    let register_user_metadata_access = Register::role(
        Role::new("ALICE_METADATA_ACCESS".parse()?)
            .add_permission(PermissionToken::new(
                "CanSetKeyValueInUserAccount".parse()?,
                &json!({ "account_id": alice_id }),
            ))
            .add_permission(PermissionToken::new(
                "CanRemoveKeyValueInUserAccount".parse()?,
                &json!({ "account_id": alice_id }),
            )),
    )
    .into();

    let parameter_defaults = ParametersBuilder::new()
        .add_parameter(
            MAX_TRANSACTIONS_IN_BLOCK,
            Numeric::new(DEFAULT_MAX_TXS.get().into(), 0),
        )?
        .add_parameter(BLOCK_TIME, Numeric::new(DEFAULT_BLOCK_TIME.as_millis(), 0))?
        .add_parameter(
            COMMIT_TIME_LIMIT,
            Numeric::new(DEFAULT_COMMIT_TIME.as_millis(), 0),
        )?
        .add_parameter(TRANSACTION_LIMITS, DEFAULT_TRANSACTION_LIMITS)?
        .add_parameter(WSV_ASSET_METADATA_LIMITS, DEFAULT_METADATA_LIMITS)?
        .add_parameter(
            WSV_ASSET_DEFINITION_METADATA_LIMITS,
            DEFAULT_METADATA_LIMITS,
        )?
        .add_parameter(WSV_ACCOUNT_METADATA_LIMITS, DEFAULT_METADATA_LIMITS)?
        .add_parameter(WSV_DOMAIN_METADATA_LIMITS, DEFAULT_METADATA_LIMITS)?
        .add_parameter(WSV_IDENT_LENGTH_LIMITS, DEFAULT_IDENT_LENGTH_LIMITS)?
        .add_parameter(
            EXECUTOR_FUEL_LIMIT,
            Numeric::new(DEFAULT_WASM_FUEL_LIMIT.into(), 0),
        )?
        .add_parameter(
            EXECUTOR_MAX_MEMORY,
            Numeric::new(DEFAULT_WASM_MAX_MEMORY_BYTES.into(), 0),
        )?
        .add_parameter(
            WASM_FUEL_LIMIT,
            Numeric::new(DEFAULT_WASM_FUEL_LIMIT.into(), 0),
        )?
        .add_parameter(
            WASM_MAX_MEMORY,
            Numeric::new(DEFAULT_WASM_MAX_MEMORY_BYTES.into(), 0),
        )?
        .into_create_parameters();

    let first_tx = genesis
        .first_transaction_mut()
        .expect("At least one transaction is expected");
    for isi in [
        mint.into(),
        mint_cabbage.into(),
        transfer_domain_ownerhip.into(),
        grant_permission_to_set_parameters.into(),
    ]
    .into_iter()
    .chain(parameter_defaults.into_iter())
    .chain(std::iter::once(register_user_metadata_access))
    {
        first_tx.append_instruction(isi);
    }

    Ok(genesis)
}

fn generate_synthetic(
    builder: RawGenesisBlockBuilder<executor_state::SetPath>,
    domains: u64,
    accounts_per_domain: u64,
    assets_per_domain: u64,
) -> color_eyre::Result<RawGenesisBlockFile> {
    // Add default `Domain` and `Account` to still be able to query
    let mut builder = builder
        .domain("wonderland".parse()?)
        .account("alice".parse()?, crate::DEFAULT_PUBLIC_KEY.parse()?)
        .finish_domain();

    for domain in 0..domains {
        let mut domain_builder = builder.domain(format!("domain_{domain}").parse()?);

        for account in 0..accounts_per_domain {
            let (public_key, _) = iroha_crypto::KeyPair::random().into_parts();
            domain_builder =
                domain_builder.account(format!("account_{account}").parse()?, public_key);
        }

        for asset in 0..assets_per_domain {
            domain_builder = domain_builder.asset(
                format!("asset_{asset}").parse()?,
                AssetValueType::Numeric(NumericSpec::default()),
            );
        }

        builder = domain_builder.finish_domain();
    }
    let mut genesis = builder.build();

    let first_transaction = genesis
        .first_transaction_mut()
        .expect("At least one transaction is expected");
    for domain in 0..domains {
        for account in 0..accounts_per_domain {
            // FIXME: it actually generates (assets_per_domain * accounts_per_domain) assets per domain
            //        https://github.com/hyperledger/iroha/issues/3508
            for asset in 0..assets_per_domain {
                let mint = Mint::asset_numeric(
                    13u32,
                    AssetId::new(
                        format!("asset_{asset}#domain_{domain}").parse()?,
                        format!("account_{account}@domain_{domain}").parse()?,
                    ),
                )
                .into();
                first_transaction.append_instruction(mint);
            }
        }
    }

    Ok(genesis)
}
