use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token::{Mint, Token, TokenAccount, Transfer},
};

declare_id!("41ggUgk3yL79W8Ue3c79gUzYSsZLpL6GDCsHt6UFYCQj");

#[program]
pub mod enhanced_auction {
    use super::*;

    pub fn initialize_auction(
        ctx: Context<InitializeAuction>,
        starting_bid: u64,
        min_bid_increment: u64,
        duration: i64,
    ) -> Result<()> {
        ctx.accounts.validate()?;
        let auction = &mut ctx.accounts.auction;
        let clock = Clock::get()?;

        require!(duration > 0, AuctionError::InvalidDuration);
        require!(starting_bid > 0, AuctionError::InvalidStartingBid);
        require!(min_bid_increment > 0, AuctionError::InvalidBidIncrement);

        auction.creator = ctx.accounts.creator.key();
        auction.nft_mint = ctx.accounts.nft_mint.key();
        auction.starting_bid = starting_bid;
        auction.min_bid_increment = min_bid_increment;
        auction.end_time = clock.unix_timestamp + duration;
        auction.highest_bid = 0;
        auction.highest_bidder = Pubkey::default();
        auction.status = AuctionStatus::Active;

        // Transfer NFT to auction vault
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.creator_nft_account.to_account_info(),
                to: ctx.accounts.vault_nft_account.to_account_info(),
                authority: ctx.accounts.creator.to_account_info(),
            },
        );
        anchor_spl::token::transfer(transfer_ctx, 1)?;

        emit!(AuctionCreated {
            auction_id: auction.key(),
            creator: auction.creator,
            nft_mint: auction.nft_mint,
            starting_bid,
            end_time: auction.end_time,
        });

        Ok(())
    }

    pub fn place_bid(ctx: Context<PlaceBid>, bid_amount: u64) -> Result<()> {
        ctx.accounts.validate()?;
        let auction = &mut ctx.accounts.auction;
        let clock = Clock::get()?;

        require!(
            clock.unix_timestamp < auction.end_time,
            AuctionError::AuctionEnded
        );
        require!(
            auction.status == AuctionStatus::Active,
            AuctionError::AuctionNotActive
        );
        require!(
            bid_amount >= auction.starting_bid,
            AuctionError::BidTooLow
        );

        if auction.highest_bid > 0 {
            require!(
                bid_amount >= auction.highest_bid + auction.min_bid_increment,
                AuctionError::BidIncrementTooLow
            );

            // Refund previous highest bidder
            let refund_ctx = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.auction_escrow.to_account_info(),
                    to: ctx.accounts.previous_bidder.to_account_info(),
                },
            );
            anchor_lang::system_program::transfer(refund_ctx, auction.highest_bid)?;
        }

        // Transfer new bid amount to escrow
        let transfer_ctx = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            anchor_lang::system_program::Transfer {
                from: ctx.accounts.bidder.to_account_info(),
                to: ctx.accounts.auction_escrow.to_account_info(),
            },
        );
        anchor_lang::system_program::transfer(transfer_ctx, bid_amount)?;

        auction.highest_bid = bid_amount;
        auction.highest_bidder = ctx.accounts.bidder.key();

        emit!(BidPlaced {
            auction_id: auction.key(),
            bidder: ctx.accounts.bidder.key(),
            bid_amount,
        });

        Ok(())
    }

    pub fn finalize_auction(ctx: Context<FinalizeAuction>) -> Result<()> {
        ctx.accounts.validate()?;
        let auction = &mut ctx.accounts.auction;
        let clock = Clock::get()?;

        require!(
            clock.unix_timestamp >= auction.end_time,
            AuctionError::AuctionNotEnded
        );
        require!(
            auction.status == AuctionStatus::Active,
            AuctionError::AuctionNotActive
        );

        auction.status = AuctionStatus::Completed;

        if auction.highest_bid > 0 {
            // Calculate platform fee (2.5%)
            let platform_fee = (auction.highest_bid * 25) / 1000;
            let seller_amount = auction.highest_bid - platform_fee;

            // Transfer funds to seller
            let seller_transfer_ctx = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.auction_escrow.to_account_info(),
                    to: ctx.accounts.creator.to_account_info(),
                },
            );
            anchor_lang::system_program::transfer(seller_transfer_ctx, seller_amount)?;

            // Transfer NFT to winner
            let nft_transfer_ctx = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.vault_nft_account.to_account_info(),
                    to: ctx.accounts.winner_nft_account.to_account_info(),
                    authority: ctx.accounts.auction_authority.to_account_info(),
                },
            );
            anchor_spl::token::transfer(nft_transfer_ctx, 1)?;

            // Transfer platform fee
            let fee_transfer_ctx = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.auction_escrow.to_account_info(),
                    to: ctx.accounts.platform_fee_account.to_account_info(),
                },
            );
            anchor_lang::system_program::transfer(fee_transfer_ctx, platform_fee)?;
        }

        emit!(AuctionFinalized {
            auction_id: auction.key(),
            winner: auction.highest_bidder,
            winning_bid: auction.highest_bid,
        });

        Ok(())
    }

    pub fn withdraw_unsold_nft(ctx: Context<WithdrawUnsoldNFT>) -> Result<()> {
        ctx.accounts.validate()?;
        let auction = &mut ctx.accounts.auction;
        let clock = Clock::get()?;

        require!(
            clock.unix_timestamp >= auction.end_time,
            AuctionError::AuctionNotEnded
        );
        require!(auction.highest_bid == 0, AuctionError::AuctionHasBids);
        require!(
            auction.status == AuctionStatus::Active,
            AuctionError::AuctionNotActive
        );

        // Transfer NFT back to creator
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault_nft_account.to_account_info(),
                to: ctx.accounts.creator_nft_account.to_account_info(),
                authority: ctx.accounts.auction_authority.to_account_info(),
            },
        );
        anchor_spl::token::transfer(transfer_ctx, 1)?;

        auction.status = AuctionStatus::Cancelled;

        emit!(AuctionCancelled {
            auction_id: auction.key(),
            reason: "No bids placed".to_string(),
        });

        Ok(())
    }

    pub fn cancel_auction(ctx: Context<CancelAuction>) -> Result<()> {
        ctx.accounts.validate()?;
        let auction = &mut ctx.accounts.auction;

        require!(
            auction.status == AuctionStatus::Active,
            AuctionError::AuctionNotActive
        );
        require!(auction.highest_bid == 0, AuctionError::AuctionHasBids);
        require_keys_eq!(
            auction.creator,
            ctx.accounts.creator.key(),
            AuctionError::UnauthorizedCancellation
        );

        // Transfer NFT back to creator
        let transfer_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.vault_nft_account.to_account_info(),
                to: ctx.accounts.creator_nft_account.to_account_info(),
                authority: ctx.accounts.auction_authority.to_account_info(),
            },
        );
        anchor_spl::token::transfer(transfer_ctx, 1)?;

        auction.status = AuctionStatus::Cancelled;

        emit!(AuctionCancelled {
            auction_id: auction.key(),
            reason: "Cancelled by creator".to_string(),
        });

        Ok(())
    }

    pub fn update_auction_settings(
        ctx: Context<UpdateAuctionSettings>,
        new_duration: Option<i64>,
        new_min_increment: Option<u64>,
    ) -> Result<()> {
        ctx.accounts.validate()?;
        let auction = &mut ctx.accounts.auction;

        require!(
            auction.status == AuctionStatus::Active,
            AuctionError::AuctionNotActive
        );
        require!(auction.highest_bid == 0, AuctionError::AuctionHasBids);
        require_keys_eq!(
            auction.creator,
            ctx.accounts.creator.key(),
            AuctionError::UnauthorizedUpdate
        );

        if let Some(duration) = new_duration {
            require!(duration > 0, AuctionError::InvalidDuration);
            let clock = Clock::get()?;
            auction.end_time = clock.unix_timestamp + duration;
        }

        if let Some(min_increment) = new_min_increment {
            require!(min_increment > 0, AuctionError::InvalidBidIncrement);
            auction.min_bid_increment = min_increment;
        }

        emit!(AuctionUpdated {
            auction_id: auction.key(),
            new_duration,
            new_min_increment,
        });

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeAuction<'info> {
    #[account(init, payer = creator, space = Auction::LEN)]
    pub auction: Account<'info, Auction>,
    #[account(mut)]
    pub creator: Signer<'info>,
    pub nft_mint: Account<'info, Mint>,
    #[account(
        mut,
        associated_token::mint = nft_mint,
        associated_token::authority = creator
    )]
    pub creator_nft_account: Account<'info, TokenAccount>,
    #[account(
        init_if_needed,
        payer = creator,
        associated_token::mint = nft_mint,
        associated_token::authority = auction
    )]
    pub vault_nft_account: Account<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct PlaceBid<'info> {
    #[account(mut)]
    pub auction: Account<'info, Auction>,
    #[account(mut)]
    pub bidder: Signer<'info>,
    /// CHECK: Previous bidder account for refund
    #[account(mut)]
    pub previous_bidder: AccountInfo<'info>,
    #[account(
        mut,
        seeds = [ESCROW_SEED, auction.key().as_ref()],
        bump,
    )]
    pub auction_escrow: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct FinalizeAuction<'info> {
    #[account(mut)]
    pub auction: Account<'info, Auction>,
    #[account(mut)]
    pub creator: SystemAccount<'info>,
    /// CHECK: Auction authority PDA
    #[account(
        mut,
        seeds = [AUCTION_SEED, auction.key().as_ref()],
        bump,
    )]
    pub auction_authority: AccountInfo<'info>,
    #[account(mut)]
    pub vault_nft_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub winner_nft_account: Account<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [ESCROW_SEED, auction.key().as_ref()],
        bump,
    )]
    pub auction_escrow: SystemAccount<'info>,
    /// CHECK: Platform fee account
    #[account(mut)]
    pub platform_fee_account: AccountInfo<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawUnsoldNFT<'info> {
    #[account(mut)]
    pub auction: Account<'info, Auction>,
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: Auction authority PDA
    #[account(
        mut,
        seeds = [AUCTION_SEED, auction.key().as_ref()],
        bump,
    )]
    pub auction_authority: AccountInfo<'info>,
    #[account(mut)]
    pub vault_nft_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub creator_nft_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelAuction<'info> {
    #[account(mut)]
    pub auction: Account<'info, Auction>,
    #[account(mut)]
    pub creator: Signer<'info>,
    /// CHECK: Auction authority PDA
    #[account(
        mut,
        seeds = [AUCTION_SEED, auction.key().as_ref()],
        bump,
    )]
    pub auction_authority: AccountInfo<'info>,
    #[account(mut)]
    pub vault_nft_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub creator_nft_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}


#[derive(Accounts)]
pub struct UpdateAuctionSettings<'info> {
    #[account(mut)]
    pub auction: Account<'info, Auction>,
    pub creator: Signer<'info>,
}

#[account]
pub struct Auction {
    pub creator: Pubkey,
    pub nft_mint: Pubkey,
    pub starting_bid: u64,
    pub min_bid_increment: u64,
    pub end_time: i64,
    pub highest_bid: u64,
    pub highest_bidder: Pubkey,
    pub status: AuctionStatus,
}

impl Auction {
    pub const LEN: usize = 8 + // discriminator
        32 + // creator
        32 + // nft_mint
        8 + // starting_bid
        8 + // min_bid_increment
        8 + // end_time
        8 + // highest_bid
        32 + // highest_bidder
        1 + // status
        200; // padding for future extensions
}


#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum AuctionStatus {
    Active,
    Completed,
    Cancelled,
}

#[error_code]
pub enum AuctionError {
    #[msg("The auction duration must be greater than 0")]
    InvalidDuration,
    #[msg("The starting bid must be greater than 0")]
    InvalidStartingBid,
    #[msg("The minimum bid increment must be greater than 0")]
    InvalidBidIncrement,
    #[msg("The auction has already ended")]
    AuctionEnded,
    #[msg("The auction is not active")]
    AuctionNotActive,
    #[msg("The bid amount is too low")]
    BidTooLow,
    #[msg("The bid increment is too low")]
    BidIncrementTooLow,
    #[msg("The auction has not ended yet")]
    AuctionNotEnded,
    #[msg("The auction already has bids")]
    AuctionHasBids,
    #[msg("Unauthorized to cancel this auction")]
    UnauthorizedCancellation,
    #[msg("Unauthorized to update this auction")]
    UnauthorizedUpdate,
    #[msg("Invalid auction state transition")]
    InvalidStateTransition,
}


#[event]
pub struct AuctionCreated {
    pub auction_id: Pubkey,
    pub creator: Pubkey,
    pub nft_mint: Pubkey,
    pub starting_bid: u64,
    pub end_time: i64,
}

#[event]
pub struct BidPlaced {
    pub auction_id: Pubkey,
    pub bidder: Pubkey,
    pub bid_amount: u64,
}

#[event]
pub struct AuctionFinalized {
    pub auction_id: Pubkey,
    pub winner: Pubkey,
    pub winning_bid: u64,
}

#[event]
pub struct AuctionCancelled {
    pub auction_id: Pubkey,
    pub reason: String,
}

#[event]
pub struct AuctionUpdated {
    pub auction_id: Pubkey,
    pub new_duration: Option<i64>,
    pub new_min_increment: Option<u64>,
}


pub const AUCTION_SEED: &[u8] = b"auction";
pub const ESCROW_SEED: &[u8] = b"escrow";
pub const VAULT_SEED: &[u8] = b"vault";


impl<'info> InitializeAuction<'info> {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

impl<'info> PlaceBid<'info> {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

impl<'info> FinalizeAuction<'info> {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

impl<'info> WithdrawUnsoldNFT<'info> {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

impl<'info> CancelAuction<'info> {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}

impl<'info> UpdateAuctionSettings<'info> {
    pub fn validate(&self) -> Result<()> {
        Ok(())
    }
}