use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Mint, Transfer, Burn, CloseAccount, InitializeAccount};
use anchor_spl::associated_token::AssociatedToken;

declare_id!("Escrow1111111111111111111111111111111111111");

#[program]
pub mod escrow {
    use super::*;

    /// Создать эскроу: создаёт аккаунт EscrowAccount и пустой vault (token account)
    /// Sender оплачивает создание (payer = sender)
    pub fn create_escrow(
        ctx: Context<CreateEscrow>,
        amount: u64,
    ) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        escrow.sender = ctx.accounts.sender.key();
        escrow.receiver = ctx.accounts.receiver.key();
        escrow.mint = ctx.accounts.mint.key();
        escrow.amount = amount;
        escrow.is_completed = false;

        // vault token account authority is the PDA (vault_authority)
        // token account already created by Anchor with token::authority pointing to vault_authority
        emit!(EscrowCreated {
            escrow: escrow.key(),
            sender: escrow.sender,
            receiver: escrow.receiver,
            mint: escrow.mint,
            amount,
        });

        Ok(())
    }

    /// Sender переводит токены со своего ATA на vault (PDA token account).
    /// Требуем, что сумма == escrow.amount (можно изменить для частичных депозитов)
    pub fn deposit_tokens(ctx: Context<DepositTokens>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        require!(!escrow.is_completed, EscrowError::AlreadyCompleted);

        // Убедимся, что отправитель совпадает
        require_keys_eq!(escrow.sender, ctx.accounts.sender.key(), EscrowError::Unauthorized);

        // require amount equals expected
        require!(
            ctx.accounts.sender_token_account.amount >= escrow.amount,
            EscrowError::InsufficientFunds
        );

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.sender_token_account.to_account_info(),
                to: ctx.accounts.vault_account.to_account_info(),
                authority: ctx.accounts.sender.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, escrow.amount)?;

        emit!(EscrowDeposited {
            escrow: escrow.key(),
            amount: escrow.amount
        });

        Ok(())
    }

    /// Receiver снимает токены из vault на свой ATA (получатель должен быть signer)
    pub fn release_tokens(ctx: Context<ReleaseTokens>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        require!(!escrow.is_completed, EscrowError::AlreadyCompleted);

        // Only receiver can call
        require_keys_eq!(escrow.receiver, ctx.accounts.receiver.key(), EscrowError::Unauthorized);

        // PDA seeds to sign for vault authority
        let seeds = &[
            b"vault-authority".as_ref(),
            escrow.key().as_ref(),
            &[ctx.accounts.vault_authority_bump],
        ];
        let signer = &[&seeds[..]];

        // transfer from vault -> receiver_token_account
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.vault_account.to_account_info(),
                to: ctx.accounts.receiver_token_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer,
        );
        token::transfer(cpi_ctx, escrow.amount)?;

        // optionally close vault to payer (receiver) to reclaim rent
        let cpi_close = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::CloseAccount {
                account: ctx.accounts.vault_account.to_account_info(),
                destination: ctx.accounts.receiver.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer,
        );
        token::close_account(cpi_close)?;

        escrow.is_completed = true;

        emit!(EscrowReleased {
            escrow: escrow.key(),
            to: escrow.receiver,
            amount: escrow.amount,
        });

        Ok(())
    }

    /// Sender отменяет эскроу и возвращает токены обратно, пока сделка не завершена.
    pub fn cancel_escrow(ctx: Context<CancelEscrow>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow_account;
        require!(!escrow.is_completed, EscrowError::AlreadyCompleted);
        require_keys_eq!(escrow.sender, ctx.accounts.sender.key(), EscrowError::Unauthorized);

        // PDA signer seeds
        let seeds = &[
            b"vault-authority".as_ref(),
            escrow.key().as_ref(),
            &[ctx.accounts.vault_authority_bump],
        ];
        let signer = &[&seeds[..]];

        // Transfer from vault back to sender_token_account
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::Transfer {
                from: ctx.accounts.vault_account.to_account_info(),
                to: ctx.accounts.sender_token_account.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer,
        );
        token::transfer(cpi_ctx, escrow.amount)?;

        // close vault, sending lamports to sender
        let cpi_close = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::CloseAccount {
                account: ctx.accounts.vault_account.to_account_info(),
                destination: ctx.accounts.sender.to_account_info(),
                authority: ctx.accounts.vault_authority.to_account_info(),
            },
            signer,
        );
        token::close_account(cpi_close)?;

        escrow.is_completed = true;

        emit!(EscrowCancelled {
            escrow: escrow.key(),
            sender: escrow.sender,
            amount: escrow.amount
        });

        Ok(())
    }
}

/// Escrow account structure
#[account]
pub struct EscrowAccount {
    pub sender: Pubkey,
    pub receiver: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub is_completed: bool,
}

#[derive(Accounts)]
#[instruction(amount: u64)]
pub struct CreateEscrow<'info> {
    #[account(init, payer = sender, space = 8 + 32*3 + 8 + 1)]
    pub escrow_account: Account<'info, EscrowAccount>,

    /// Vault account: token account that will hold tokens with authority = vault_authority PDA
    /// We initialize token account for the given mint, authority = vault_authority PDA (see seeds)
    #[account(
        init,
        payer = sender,
        token::mint = mint,
        token::authority = vault_authority,
        seeds = [b"vault", escrow_account.key().as_ref()],
        bump,
    )]
    pub vault_account: Account<'info, TokenAccount>,

    /// The PDA that will be the authority for the vault token account (non-token account)
    /// This is not stored on-chain, we pass it as an AccountInfo (unchecked)
    /// but Anchor will compute bump and we will use it for signing.
    /// We also include it to prevent accidental mismatches at runtime.
    /// vault_authority is a PDA (no data stored) and is of type SystemAccount (unchecked)
    /// The bump is later passed in contexts as vault_authority_bump
    #[account(seeds = [b"vault-authority", escrow_account.key().as_ref()], bump)]
    /// CHECK: This is a PDA used as authority for the token account
    pub vault_authority: UncheckedAccount<'info>,

    /// the mint of the token being escrowed
    pub mint: Account<'info, Mint>,

    #[account(mut)]
    pub sender: Signer<'info>,

    /// who will receive funds
    /// CHECK: not signer at creation
    pub receiver: UncheckedAccount<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
    pub rent: Sysvar<'info, Rent>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

#[derive(Accounts)]
pub struct DepositTokens<'info> {
    #[account(mut, has_one = sender)]
    pub escrow_account: Account<'info, EscrowAccount>,

    #[account(mut)]
    pub sender: Signer<'info>,

    /// Sender's token account (ATA) for the mint
    #[account(mut, token::mint = escrow_account.mint, constraint = sender_token_account.owner == sender.key() )]
    pub sender_token_account: Account<'info, TokenAccount>,

    /// The vault token account
    #[account(mut, seeds = [b"vault", escrow_account.key().as_ref()], bump)]
    pub vault_account: Account<'info, TokenAccount>,

    /// Vault authority PDA - used as token authority for vault_account
    #[account(seeds = [b"vault-authority", escrow_account.key().as_ref()], bump)]
    /// CHECK:
    pub vault_authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ReleaseTokens<'info> {
    #[account(mut, has_one = receiver)]
    pub escrow_account: Account<'info, EscrowAccount>,

    /// Receiver must sign
    #[account(mut)]
    pub receiver: Signer<'info>,

    /// Receiver's token account (ATA) for mint
    #[account(mut, token::mint = escrow_account.mint, constraint = receiver_token_account.owner == receiver.key() )]
    pub receiver_token_account: Account<'info, TokenAccount>,

    /// Vault token account
    #[account(mut, seeds = [b"vault", escrow_account.key().as_ref()], bump)]
    pub vault_account: Account<'info, TokenAccount>,

    /// Vault authority PDA
    /// We pass bump as u8 in ctx for signing
    #[account(seeds = [b"vault-authority", escrow_account.key().as_ref()], bump)]
    /// CHECK:
    pub vault_authority: UncheckedAccount<'info>,

    /// Bumps are available in the derived Accounts object - but Anchor does not automatically inject bump value
    /// To get the bump within instruction, include it in the call as extra field or read from accounts.
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CancelEscrow<'info> {
    #[account(mut, has_one = sender)]
    pub escrow_account: Account<'info, EscrowAccount>,

    #[account(mut)]
    pub sender: Signer<'info>,

    /// Sender's token account to receive refund
    #[account(mut, token::mint = escrow_account.mint, constraint = sender_token_account.owner == sender.key())]
    pub sender_token_account: Account<'info, TokenAccount>,

    /// Vault token account
    #[account(mut, seeds = [b"vault", escrow_account.key().as_ref()], bump)]
    pub vault_account: Account<'info, TokenAccount>,

    /// Vault authority PDA
    #[account(seeds = [b"vault-authority", escrow_account.key().as_ref()], bump)]
    /// CHECK:
    pub vault_authority: UncheckedAccount<'info>,

    pub token_program: Program<'info, Token>,
}

/// Events
#[event]
pub struct EscrowCreated {
    pub escrow: Pubkey,
    pub sender: Pubkey,
    pub receiver: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
}

#[event]
pub struct EscrowDeposited {
    pub escrow: Pubkey,
    pub amount: u64,
}

#[event]
pub struct EscrowReleased {
    pub escrow: Pubkey,
    pub to: Pubkey,
    pub amount: u64,
}

#[event]
pub struct EscrowCancelled {
    pub escrow: Pubkey,
    pub sender: Pubkey,
    pub amount: u64,
}

#[error_code]
pub enum EscrowError {
    #[msg("Escrow is already completed")]
    AlreadyCompleted,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Insufficient funds")]
    InsufficientFunds,
}