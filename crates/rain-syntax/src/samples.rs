//! Sample sources for tests, benches and the example: (language, code).

pub const CSHARP: &str = r#"using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.AspNetCore.Mvc;

namespace Acme.Api.Controllers
{
    /* Vouchers recorded against an account.
       Amounts are stored in the company's currency. */
    [ApiController]
    [Route("api/accounts/{accountId:guid}/vouchers")]
    public sealed class VouchersController : ControllerBase
    {
        private readonly IVoucherService _vouchers;
        private readonly Dictionary<string, List<decimal>> _cache = new();

        public VouchersController(IVoucherService vouchers)
        {
            _vouchers = vouchers ?? throw new ArgumentNullException(nameof(vouchers));
        }

        [HttpGet]
        [ProducesResponseType(typeof(IEnumerable<VoucherViewModel>), 200)]
        public async Task<IActionResult> List(Guid accountId, int page = 1)
        {
            var vouchers = await _vouchers.ForAccount(accountId);
            var visible = vouchers
                .Where(v => v.Amount > 0m && !v.IsVoided)
                .OrderByDescending(v => v.CreatedAt)
                .Skip((page - 1) * 25)
                .Take(25)
                .Select(v => new VoucherViewModel(v.Id, v.Amount, v.ReaderName))
                .ToList();
            return Ok(visible);
        }

        [HttpPost]
        public async Task<ActionResult<VoucherViewModel>> Create(Guid accountId, [FromBody] CreateVoucherRequest request)
        {
            if (request.Amount <= 0)
            {
                return BadRequest($"Voucher for {accountId} must be positive, got {request.Amount:C}");
            }
            string label = request.Note?.Trim() ?? "voucher";
            var voucher = await _vouchers.Record(accountId, request.Amount, label);
            return CreatedAtAction(nameof(List), new { accountId }, voucher);
        }

        // Totals per reader for the summary card.
        public IReadOnlyDictionary<string, decimal> Totals(IEnumerable<Voucher> vouchers) =>
            vouchers.GroupBy(v => v.ReaderName)
                .ToDictionary(g => g.Key, g => g.Sum(v => v.Amount));

        private static bool IsRound(decimal amount) => amount % 1 == 0;
    }

    public record VoucherViewModel(Guid Id, decimal Amount, string ReaderName);
}
"#;

pub const TYPESCRIPT: &str = r#"import { Component, inject, signal } from '@angular/core';
import type { VoucherViewModel } from './models';

// Shows the vouchers recorded for one account.
@Component({ selector: 'app-vouchers', templateUrl: './vouchers.component.html' })
export class VouchersComponent {
  private readonly api = inject(VouchersApi);
  readonly vouchers = signal<VoucherViewModel[]>([]);
  readonly total = () => this.vouchers().reduce((sum, v) => sum + v.amount, 0);

  async load(accountId: string, page = 1): Promise<void> {
    const rows = await this.api.list(accountId, { page });
    this.vouchers.set(rows.filter((v) => v.amount > 0));
    console.log(`loaded ${rows.length} vouchers`);
  }
}
"#;

pub const SCSS: &str = r#"@use 'variables' as v;

.vouchers-card {
  display: flex;
  gap: 0.5rem;
  color: v.$text-base;
  // Muted when there is nothing to show.
  &--empty { opacity: 0.6; }
  &:hover > .amount { font-weight: 600; }
}
"#;

pub const POWERSHELL: &str = r#"param([string]$Story = 'ACME-1234', [switch]$Native)

# Opens each repo of the story in the viewer.
$repos = Get-ChildItem -Directory C:\src | Where-Object { Test-Path (Join-Path $_.FullName '.git') }
foreach ($repo in $repos) {
    $branch = git -C $repo.FullName branch --show-current
    if ($branch -like "*$Story*") {
        Write-Host "opening $($repo.Name) at $branch" -ForegroundColor Cyan
    }
}
"#;

pub const SQL: &str = r#"-- Vouchers per reader over the last 30 days.
SELECT c.Name, SUM(v.Amount) AS Total, COUNT(*) AS Vouchers
FROM dbo.Vouchers v
INNER JOIN dbo.Readers c ON c.Id = v.ReaderId
WHERE v.CreatedAt >= DATEADD(day, -30, GETUTCDATE()) AND v.IsVoided = 0
GROUP BY c.Name
ORDER BY Total DESC;
"#;

pub const MARKDOWN: &str = r#"# Vouchers

Members can redeem **vouchers** at checkout.

- Stored per account in `dbo.Vouchers`
- See [the story](https://example.com/ACME-1234)

```cs
var total = vouchers.Sum(v => v.Amount);
```
"#;

pub const ALL: [(&str, &str); 6] = [
    ("cs", CSHARP),
    ("ts", TYPESCRIPT),
    ("scss", SCSS),
    ("ps1", POWERSHELL),
    ("sql", SQL),
    ("md", MARKDOWN),
];
