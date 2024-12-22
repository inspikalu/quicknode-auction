import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import assert from "assert";
import { AuctionSystem } from "../target/types/auction_system";

// Utility function for creating keypairs
const createKeypair = async (): Promise<anchor.web3.Keypair> => {
  const keypair = anchor.web3.Keypair.generate();
  return keypair;
};

describe("Auction System Tests", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.AuctionSystem as Program<AuctionSystem>;

  // Accounts
  let auctionAccount: anchor.web3.Keypair;
  let nftMint: anchor.web3.Keypair;
  let nftVault: anchor.web3.PublicKey;
  let bidder: anchor.web3.Keypair;
  let creator: anchor.web3.Keypair;

  before(async () => {
    // Initialize keypairs
    auctionAccount = await createKeypair();
    nftMint = await createKeypair();
    creator = await createKeypair();
    bidder = await createKeypair();

    // Airdrop SOL to creator and bidder
    const airdropTx1 = await provider.connection.requestAirdrop(
      creator.publicKey,
      anchor.web3.LAMPORTS_PER_SOL * 2
    );
    const airdropTx2 = await provider.connection.requestAirdrop(
      bidder.publicKey,
      anchor.web3.LAMPORTS_PER_SOL * 2
    );
    await provider.connection.confirmTransaction(airdropTx1);
    await provider.connection.confirmTransaction(airdropTx2);
  });

  it("Initializes an auction", async () => {
    nftVault = await anchor.web3.PublicKey.findProgramAddress(
      [Buffer.from("vault"), auctionAccount.publicKey.toBuffer()],
      program.programId
    ).then(([address]) => address);

    await program.methods
      .initializeAuction(
        new anchor.BN(anchor.web3.LAMPORTS_PER_SOL / 10), // Starting bid
        new anchor.BN(anchor.web3.LAMPORTS_PER_SOL / 20), // Minimum increment
        new anchor.BN(60 * 60) // Auction duration
      )
      .accounts({
        auction: auctionAccount.publicKey,
        creator: creator.publicKey,
        vaultNftAccount: nftVault,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([auctionAccount, creator])
      .rpc();

    const auctionState = await program.account.auction.fetch(
      auctionAccount.publicKey
    );
    assert.strictEqual(auctionState.creator.toBase58(), creator.publicKey.toBase58());
    assert.strictEqual(auctionState.status.active, true);
  });

  it("Places a valid bid", async () => {
    await program.methods
      .placeBid(new anchor.BN(anchor.web3.LAMPORTS_PER_SOL / 2))
      .accounts({
        auction: auctionAccount.publicKey,
        bidder: bidder.publicKey,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([bidder])
      .rpc();

    const auctionState = await program.account.auction.fetch(
      auctionAccount.publicKey
    );
    assert.strictEqual(auctionState.highestBidder.toBase58(), bidder.publicKey.toBase58());
    assert.strictEqual(
      auctionState.highestBid.toNumber(),
      anchor.web3.LAMPORTS_PER_SOL / 2
    );
  });

  it("Fails to place a bid lower than the current highest bid", async () => {
    try {
      await program.methods
        .placeBid(new anchor.BN(anchor.web3.LAMPORTS_PER_SOL / 4))
        .accounts({
          auction: auctionAccount.publicKey,
          bidder: bidder.publicKey,
          systemProgram: anchor.web3.SystemProgram.programId,
        })
        .signers([bidder])
        .rpc();
      assert.fail("Bid should not have succeeded");
    } catch (err) {
      assert.match(err.message, /Bid must exceed current highest bid/);
    }
  });

  it("Finalizes the auction", async () => {
    await program.methods
      .finalizeAuction()
      .accounts({
        auction: auctionAccount.publicKey,
        creator: creator.publicKey,
        vaultNftAccount: nftVault,
        highestBidder: bidder.publicKey,
      })
      .signers([creator])
      .rpc();

    const auctionState = await program.account.auction.fetch(
      auctionAccount.publicKey
    );
    assert.strictEqual(auctionState.status.completed, true);
  });

  it("Fails to finalize an already completed auction", async () => {
    try {
      await program.methods
        .finalizeAuction()
        .accounts({
          auction: auctionAccount.publicKey,
          creator: creator.publicKey,
          vaultNftAccount: nftVault,
          highestBidder: bidder.publicKey,
        })
        .signers([creator])
        .rpc();
      assert.fail("Finalization should not have succeeded");
    } catch (err) {
      assert.match(err.message, /Auction is already completed/);
    }
  });

  it("Withdraws an unsold NFT", async () => {
    await program.methods
      .withdrawUnsoldNft()
      .accounts({
        auction: auctionAccount.publicKey,
        creator: creator.publicKey,
        vaultNftAccount: nftVault,
      })
      .signers([creator])
      .rpc();

    const auctionState = await program.account.auction.fetch(
      auctionAccount.publicKey
    );
    assert.strictEqual(auctionState.status.cancelled, true);
  });

  it("Cancels an auction before any bids are placed", async () => {
    const newAuction = await createKeypair();
    await program.methods
      .initializeAuction(
        new anchor.BN(anchor.web3.LAMPORTS_PER_SOL / 10),
        new anchor.BN(anchor.web3.LAMPORTS_PER_SOL / 20),
        new anchor.BN(60 * 60)
      )
      .accounts({
        auction: newAuction.publicKey,
        creator: creator.publicKey,
        vaultNftAccount: nftVault,
        systemProgram: anchor.web3.SystemProgram.programId,
      })
      .signers([newAuction, creator])
      .rpc();

    await program.methods
      .cancelAuction()
      .accounts({
        auction: newAuction.publicKey,
        creator: creator.publicKey,
      })
      .signers([creator])
      .rpc();

    const auctionState = await program.account.auction.fetch(
      newAuction.publicKey
    );
    assert.strictEqual(auctionState.status.cancelled, true);
  });
});