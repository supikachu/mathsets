-- sqlx disable_transaction
ALTER TYPE space_role ADD VALUE IF NOT EXISTS 'member';
