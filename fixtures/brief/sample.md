# Vouchers at checkout

## Intent

Adds an optional voucher to account payments (AC1–AC4). Vouchers are recorded with the payment and included in account
totals. The API takes it at `server/src/Acme.Api/Controllers/Voucher2Controller.cs:20-27`.

## How it works

### The API records a voucher

`POST /voucher` validates the amount and records one voucher per payment.

### One Voucher row in the same unit of work

The voucher row and the payment are saved in the same unit of work, so a failed payment never leaves a voucher behind.

### The form shows the voucher

The voucher form adds an amount field; the account screen shows vouchers in the total.

## Invariants

- A voucher is never more than the account total.
  Enforced: `server/src/Acme.Api/Controllers/Voucher2Controller.cs:23`
  Tested: `app/src/app/voucher1/services/voucher1.service.spec.ts`

## Not changed

Refunds and inventory reports are untouched.

## Not covered

Generated i18n files and form styles.

```paths
app/src/assets/i18n/*.json
app/src/app/**/*.scss
```

## Verify by hand

Pay an account with a voucher of 0, a negative voucher and a voucher above the total.

## Tour

1. **The endpoint** `server/src/Acme.Api/Controllers/Voucher2Controller.cs:20-27` Start where a voucher enters the system.
2. **The schema** `server/src/Acme.Data/Migrations/20260101000000_AddVoucherColumns.cs:1-30` The migration adds the voucher columns.
