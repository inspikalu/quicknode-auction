import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import { EnhancedAuction } from "../target/types/enhanced_auction";
import { PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { 
  TOKEN_PROGRAM_ID, 
  ASSOCIATED_TOKEN_PROGRAM_ID,
  createMint,
  createAssociatedTokenAccount,
  mintTo
} from "@solana/spl-token";
import { assert } from "chai";

describe("enhanced_auction", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.EnhancedAuction as Program<EnhancedAuction>;
  
  let nftMint: PublicKey;
  let creatorNftAccount: PublicKey;
  let vaultNftAccount: PublicKey;
  let auction: anchor.web3.Keypair;
  let creator: anchor.web3.Keypair;
  let bidder: anchor.web3.Keypair;
  let winnerNftAccount: PublicKey;
  let auctionAuthority: PublicKey;
  let auctionAuthorityBump: number;
  let escrowAccount: PublicKey;
  let escrowBump: number;

  before(async () => {
    creator = anchor.web3.Keypair.generate();
    bidder = anchor.web3.Keypair.generate();
    auction = anchor.web3.Keypair.generate();

    // Airdrop SOL to creator and bidder
    await provider.connection.requestAirdrop(creator.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);
    await provider.connection.requestAirdrop(bidder.publicKey, 10 * anchor.web3.LAMPORTS_PER_SOL);

    // Create NFT mint
    nftMint = await createMint(
      provider.connection,
      creator,
      creator.publicKey,
      null,
      0
    );

    // Create token accounts
    creatorNftAccount = await createAssociatedTokenAccount(
      provider.connection,
      creator,
      nftMint,
      creator.publicKey
    );

    // Mint NFT to creator
    await mintTo(
      provider.connection,
      creator,
      nftMint,
      creatorNftAccount,
      creator,
      1
    );

    // Derive PDAs
    [auctionAuthority, auctionAuthorityBump] = await PublicKey.findProgramAddress(
      [Buffer.from("auction"), auction.publicKey.toBuffer()],
      program.programId
    );

    [escrowAccount, escrowBump] = await PublicKey.findProgramAddress(
      [Buffer.from("escrow"), auction.publicKey.toBuffer()],
      program.programId
    );

    // Create winner's token account
    winnerNftAccount = await createAssociatedTokenAccount(
      provider.connection,
      bidder,
      nftMint,
      bidder.publicKey
    );

    vaultNftAccount = await createAssociatedTokenAccount(
      provider.connection,
      creator,
      nftMint,
      auctionAuthority,
      true
    );
  });

  describe("Initialize Auction", () => {
    it("Successfully initializes auction", async () => {
      await program.methods
        .initializeAuction(
          new anchor.BN(1_000_000), // 1 SOL starting bid
          new anchor.BN(100_000),   // 0.1 SOL min increment
          new anchor.BN(3600)       // 1 hour duration
        )
        .accounts({
          auction: auction.publicKey,
          creator: creator.publicKey,
          nftMint: nftMint,
          creatorNftAccount: creatorNftAccount,
          vaultNftAccount: vaultNftAccount,
          systemProgram: SystemProgram.programId,
          tokenProgram: TOKEN_PROGRAM_ID,
          associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
          rent: SYSVAR_RENT_PUBKEY,
        })
        .signers([auction, creator])
        .rpc();

      const auctionAccount = await program.account.auction.fetch(auction.publicKey);
      assert.equal(auctionAccount.creator.toString(), creator.publicKey.toString());
      assert.equal(auctionAccount.status.active !== undefined, true);
    });

    it("Fails with invalid duration", async () => {
      const newAuction = anchor.web3.Keypair.generate();
      try {
        await program.methods
          .initializeAuction(
            new anchor.BN(1_000_000),
            new anchor.BN(100_000),
            new anchor.BN(0) // Invalid duration
          )
          .accounts({
            auction: newAuction.publicKey,
            creator: creator.publicKey,
            nftMint: nftMint,
            creatorNftAccount: creatorNftAccount,
            vaultNftAccount: vaultNftAccount,
            systemProgram: SystemProgram.programId,
            tokenProgram: TOKEN_PROGRAM_ID,
            associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            rent: SYSVAR_RENT_PUBKEY,
          })
          .signers([newAuction, creator])
          .rpc();
        assert.fail("Should have failed with invalid duration");
      } catch (err) {
        assert.include(err.message, "InvalidDuration");
      }
    });
  });

  describe("Place Bid", () => {
    it("Successfully places first bid", async () => {
      await program.methods
        .placeBid(new anchor.BN(1_500_000))
        .accounts({
          auction: auction.publicKey,
          bidder: bidder.publicKey,
          previousBidder: SystemProgram.programId, // No previous bidder
          auctionEscrow: escrowAccount,
          systemProgram: SystemProgram.programId,
        })
        .signers([bidder])
        .rpc();

      const auctionAccount = await program.account.auction.fetch(auction.publicKey);
      assert.equal(auctionAccount.highestBid.toString(), "1500000");
      assert.equal(auctionAccount.highestBidder.toString(), bidder.publicKey.toString());
    });

    it("Fails with bid below minimum increment", async () => {
      const newBidder = anchor.web3.Keypair.generate();
      await provider.connection.requestAirdrop(newBidder.publicKey, 2 * anchor.web3.LAMPORTS_PER_SOL);

      try {
        await program.methods
          .placeBid(new anchor.BN(1_550_000)) // Below minimum increment
          .accounts({
            auction: auction.publicKey,
            bidder: newBidder.publicKey,
            previousBidder: bidder.publicKey,
            auctionEscrow: escrowAccount,
            systemProgram: SystemProgram.programId,
          })
          .signers([newBidder])
          .rpc();
        assert.fail("Should have failed with bid increment too low");
      } catch (err) {
        assert.include(err.message, "BidIncrementTooLow");
      }
    });
  });

  describe("Finalize Auction", () => {
    it("Fails to finalize before end time", async () => {
      try {
        await program.methods
          .finalizeAuction()
          .accounts({
            auction: auction.publicKey,
            creator: creator.publicKey,
            auctionAuthority: auctionAuthority,
            vaultNftAccount: vaultNftAccount,
            winnerNftAccount: winnerNftAccount,
            auctionEscrow: escrowAccount,
            platformFeeAccount: provider.wallet.publicKey,
            tokenProgram: TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          })
          .rpc();
        assert.fail("Should have failed with auction not ended");
      } catch (err) {
        assert.include(err.message, "AuctionNotEnded");
      }
    });

    // Add more test cases for successful finalization after auction ends
  });

  describe("Cancel Auction", () => {
    it("Fails to cancel auction with bids", async () => {
      try {
        await program.methods
          .cancelAuction()
          .accounts({
            auction: auction.publicKey,
            creator: creator.publicKey,
            auctionAuthority: auctionAuthority,
            vaultNftAccount: vaultNftAccount,
            creatorNftAccount: creatorNftAccount,
            tokenProgram: TOKEN_PROGRAM_ID,
          })
          .signers([creator])
          .rpc();
        assert.fail("Should have failed with auction has bids");
      } catch (err) {
        assert.include(err.message, "AuctionHasBids");
      }
    });
  });

  // Add more test cases for other instructions
});