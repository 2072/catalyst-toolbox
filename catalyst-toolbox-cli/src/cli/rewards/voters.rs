use super::Error;
use jcli_lib::jcli_lib::block::Common;

use structopt::StructOpt;

use chain_addr::{Discrimination, Kind};
use chain_impl_mockchain::vote::CommitteeId;
use jormungandr_lib::interfaces::{Address, Block0Configuration, Initial};
use std::collections::{HashMap, HashSet};
use std::ops::{Div, Mul};

const ADA_TO_LOVELACE_FACTOR: u64 = 1_000_000;

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct VotersRewards {
    #[structopt(flatten)]
    common: Common,
    /// Reward (in LOVELACE) to be distributed
    #[structopt(long = "total-rewards")]
    total_rewards: u64,
}

fn calculate_stake<'address>(
    committee_keys: &HashSet<Address>,
    block0: &'address Block0Configuration,
) -> (u64, HashMap<&'address Address, u64>) {
    let mut total_stake: u64 = 0;
    let mut stake_per_voter: HashMap<&'address Address, u64> = HashMap::new();

    for fund in &block0.initial {
        match fund {
            Initial::Fund(fund) => {
                for utxo in fund {
                    if !committee_keys.contains(&utxo.address) {
                        let value: u64 = utxo.value.into();
                        total_stake += value;
                        let entry = stake_per_voter.entry(&utxo.address).or_default();
                        *entry += value;
                    }
                }
            }
            Initial::Cert(_) => {}
            Initial::LegacyFund(_) => {}
        }
    }
    (total_stake, stake_per_voter)
}

/// Rewards are u64 for keeping the it we would calculate the inverse total_stake/voter_stake
fn calculate_inverse_reward_share<'address>(
    total_stake: u64,
    stake_per_voter: &HashMap<&'address Address, u64>,
) -> HashMap<&'address Address, u64> {
    stake_per_voter
        .iter()
        .map(|(k, v)| (*k, total_stake.div(v)))
        .collect()
}

/// caculate total reward from the inverse of the the reward share
fn calculate_total_reward(share: u64, total_reward: u64) -> f64 {
    1f64.div(share as f64).mul(total_reward as f64)
}

fn write_rewards_results(
    common: Common,
    stake_per_voter: &HashMap<&Address, u64>,
    results: &HashMap<&Address, u64>,
    total_rewards: u64,
) -> Result<(), Error> {
    let writer = common.open_output()?;
    let header = [
        "Address",
        "Stake of the voter",
        "Reward for the voter (ADA)",
        "Reward for the voter (lovelace)",
    ];
    let mut csv_writer = csv::Writer::from_writer(writer);
    csv_writer.write_record(&header).map_err(Error::Csv)?;

    for (address, reward) in results.iter() {
        let stake = stake_per_voter.get(*address).unwrap();
        let voter_reward = calculate_total_reward(*reward, total_rewards);
        let record = [
            address.to_string(),
            stake.to_string(),
            format!("{:.8}", voter_reward), // transform lovelace to ADA (// 1000000)
            format!(
                "{:.2}",
                voter_reward
                    .mul(ADA_TO_LOVELACE_FACTOR as f64)
                    .trunc()
                    .to_string()
            ),
        ];
        csv_writer.write_record(&record).map_err(Error::Csv)?;
    }
    Ok(())
}

impl VotersRewards {
    pub fn exec(self) -> Result<(), Error> {
        let VotersRewards {
            common,
            total_rewards,
        } = self;
        let block = common.input.load_block()?;
        let block0 = Block0Configuration::from_block(&block)
            .map_err(jcli_lib::jcli_lib::block::Error::BuildingGenesisFromBlock0Failed)?;
        let committee_keys: HashSet<Address> = block0
            .blockchain_configuration
            .committees
            .iter()
            .cloned()
            .map(|id| {
                let id = CommitteeId::from(id);
                let pk = id.public_key();

                chain_addr::Address(Discrimination::Production, Kind::Account(pk)).into()
            })
            .collect();

        let (total_stake, stake_per_voter) = calculate_stake(&committee_keys, &block0);
        let rewards =
            calculate_inverse_reward_share(total_stake * ADA_TO_LOVELACE_FACTOR, &stake_per_voter);
        write_rewards_results(common, &stake_per_voter, &rewards, total_rewards)?;
        Ok(())
    }
}
