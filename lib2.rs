use anchor_lang::prelude::*;
use anchor_spl::token::TokenAccount;
use solana_program_test::*;
use solana_sdk::{signature::Keypair, signer::Signer};
use my_token::program::MyToken;

#[tokio::test]
async fn test_burn_tokens() {
    // Запускаем тестовую среду
    let program = ProgramTest::new("my_token", my_token::id(), processor!(my_token::entry));
    let (mut banks_client, payer, recent_blockhash) = program.start().await;

    // создаем пользователя и его token account (см. твои прошлые тесты)
    // ...
    // минтим, например, 1000 токенов на его счет
    // ...

    // 🔥 теперь сжигаем 400 токенов
    let burn_amount = 400u64;

    let tx = Transaction::new_signed_with_payer(
        &[Instruction {
            program_id: my_token::id(),
            accounts: vec![
                AccountMeta::new(mint_pda, false),
                AccountMeta::new(user_token_account, false),
                AccountMeta::new_readonly(payer.pubkey(), true),
                AccountMeta::new_readonly(spl_token::id(), false),
            ],
            data: my_token::instruction::BurnTokens { amount: burn_amount }.data(),
        }],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    banks_client.process_transaction(tx).await.unwrap();

    // ✅ Проверяем, что баланс уменьшился
    let account_data = banks_client
        .get_account(user_token_account)
        .await
        .unwrap()
        .unwrap();
    let token_acc: TokenAccount = TokenAccount::try_deserialize(&mut account_data.data.as_ref()).unwrap();
    assert_eq!(token_acc.amount, 600);
}