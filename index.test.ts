import * as anchor from "@project-serum/anchor";
import { Program } from "@project-serum/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, getAccount, getOrCreateAssociatedTokenAccount } from "@solana/spl-token";
import { assert } from "chai";

describe("MyToken program tests", () => {
  // Подключаем провайдера
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);

  const program = anchor.workspace.MyToken as Program;

  let mint: PublicKey;
  let userTokenAccount: PublicKey;
  let user2TokenAccount: PublicKey;
  const user = provider.wallet.publicKey;

  it("create_token()", async () => {
    const [mintPda] = await PublicKey.findProgramAddress(
      [Buffer.from("mint")],
      program.programId
    );

    await program.methods
      .createToken()
      .accounts({
        payer: user,
        mint: mintPda,
        systemProgram: SystemProgram.programId,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    mint = mintPda;

    const mintInfo = await program.provider.connection.getAccountInfo(mint);
    assert.ok(mintInfo !== null);
    console.log("✅ Mint создан:", mint.toBase58());
  });

  it("create_token_account()", async () => {
    // Создаем ATA для пользователя
    const ata = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      provider.wallet.payer,
      mint,
      user
    );

    userTokenAccount = ata.address;

    assert.ok(userTokenAccount !== null);
    console.log("✅ Token account создан:", userTokenAccount.toBase58());
  });

  it("mint_tokens()", async () => {
    const amount = new anchor.BN(1000);

    await program.methods
      .mintTokens(amount)
      .accounts({
        mint,
        tokenAccount: userTokenAccount,
        authority: user,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const accountInfo = await getAccount(provider.connection, userTokenAccount);
    assert.strictEqual(Number(accountInfo.amount), 1000);
    console.log("✅ На аккаунт наминтили:", accountInfo.amount.toString());
  });

  it("transfer_tokens()", async () => {
    // создаем 2-го пользователя
    const user2 = anchor.web3.Keypair.generate();

    // создаем ATA для user2
    const ata2 = await getOrCreateAssociatedTokenAccount(
      provider.connection,
      provider.wallet.payer,
      mint,
      user2.publicKey
    );

    user2TokenAccount = ata2.address;

    const amountToTransfer = new anchor.BN(500);

    await program.methods
      .transferTokens(amountToTransfer)
      .accounts({
        from: userTokenAccount,
        to: user2TokenAccount,
        authority: user,
        tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();

    const fromAccountInfo = await getAccount(provider.connection, userTokenAccount);
    const toAccountInfo = await getAccount(provider.connection, user2TokenAccount);

    assert.strictEqual(Number(fromAccountInfo.amount), 500);
    assert.strictEqual(Number(toAccountInfo.amount), 500);

    console.log("✅ Перевод выполнен: user =", fromAccountInfo.amount.toString(), " user2 =", toAccountInfo.amount.toString());
  });
});