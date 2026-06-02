# StellarLend Oracle Service

Off-chain oracle integration service that fetches price data from multiple external sources and updates the smart contract on Soroban.

## Features

- **Multi-Source Price Fetching**: Aggregates prices from CoinGecko and Binance
- **Price Validation**: Validates prices for staleness, deviation, and bounds
- **Weighted Median**: Calculates weighted median from multiple sources for accuracy
- **MAD Outlier Rejection**: Filters rogue/broken feed prices before aggregation using Median Absolute Deviation
- **Efficient Caching**: In-memory caching with configurable TTL to reduce API calls

## Prerequisites

- Node.js >= 18.0.0
- npm

## Installation

```bash
cd oracle
npm install
```

## Configuration

Copy the example environment file and configure:

```bash
cp .env.example .env
```

### Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `STELLAR_NETWORK` | Network: `testnet` or `mainnet` | Yes |
| `STELLAR_RPC_URL` | Soroban RPC endpoint | Yes |
| `CONTRACT_ID` | StellarLend contract address | Yes |
| `ADMIN_SECRET_KEY` | Secret key for signing transactions | Yes |
| `COINGECKO_API_KEY` | CoinGecko Pro API key | No |
| `CACHE_TTL_SECONDS` | Cache TTL in seconds (default: 30) | No |
| `UPDATE_INTERVAL_MS` | Price update interval (default: 60000) | No |
| `MAX_PRICE_DEVIATION_PERCENT` | Max price deviation % (default: 10) | No |
| `MAD_Z_SCORE_THRESHOLD` | MAD outlier filter z-score (default: 3.5, 0 = disabled) | No |
| `LOG_LEVEL` | Logging: debug, info, warn, error | No |

## Usage

### Development

```bash
npm run dev
```

### Production

```bash
npm run build
npm start
```

### Testing

```bash
npm test                 # Run all tests
npm run test:coverage    # With coverage report
npm run test:watch       # Watch mode
```

## Live Integration Test

To verify proper operation with real APIs (CoinGecko, Binance), run the live test script:

```bash
npx tsx tests/live-test.ts
```

This script will:
1. Initialize the CoinGecko and Binance providers.
2. Fetch live prices for XLM and BTC from each.
3. Aggregate the prices and display the result.

## Supported Assets

| Asset | CoinGecko | Binance |
|-------|-----------|---------|
| XLM   | Yes       | Yes     |
| USDC  | Yes       | Yes     |
| BTC   | Yes       | Yes     |
| ETH   | Yes       | Yes     |
| SOL   | Yes       | Yes     |

## MAD Outlier Rejection

Before the weighted-median is computed, `filterOutliersByMAD` removes prices from
broken or malicious feeds using the **Median Absolute Deviation** method.

### Algorithm

For a set of scaled bigint prices `p₁ … pₙ`:

1. Compute the sample median `M`.
2. Compute `MAD = median(|pᵢ − M|)`.
3. Compute the modified z-score for each price:
   ```
   zᵢ = |pᵢ − M| / (1.4826 × MAD)
   ```
   The constant `1.4826` makes MAD a consistent estimator of σ under Gaussian noise.
4. Reject any price where `zᵢ > zMax`.

If filtering would leave fewer sources than `minSources`, the full unfiltered list is used as a safe fallback so the oracle never silently stalls.

### When filtering is skipped

- **≤ 2 prices** — not enough data to distinguish signal from noise; all prices are kept.
- **MAD = 0** — all prices are identical; nothing to reject.
- **zMax ≤ 0** — filtering is explicitly disabled.

### Configuration

| Parameter | Env var | Default | Effect |
|-----------|---------|---------|--------|
| `madZScoreThreshold` | `MAD_Z_SCORE_THRESHOLD` | `3.5` | Prices with modified z-score above this are rejected. Lower = stricter. |

A threshold of **3.5** is the value recommended by Iglewicz & Hoaglin (1993) for detecting outliers in small samples. For tighter protection lower it to `2.5`; set to `0` to disable entirely.

### Example

With prices `[100, 101, 102, 5000]` and `zMax = 3.5`:

- Median = 101.5, MAD = 1, modified z-score of 5000 ≈ 3290 → **rejected**.
- Output: `[100, 101, 102]`

## Price Sources

### CoinGecko (Primary)
- Popular crypto price API
- Priority: 1, Weight: 60%

### Binance (Secondary)
- Public market data API
- Priority: 2, Weight: 40%

## Programmatic Usage

```typescript
import { OracleService, loadConfig } from 'stellarlend-oracle';

const config = loadConfig();
const service = new OracleService(config);

// Start automatic updates
await service.start(['XLM', 'USDC', 'BTC']);

// Or fetch manually
const price = await service.fetchPrice('XLM');

// Stop service
service.stop();
```

## Project Structure

```
oracle/
├── src/
│   ├── index.ts              # Main entry point
│   ├── config.ts             # Configuration
│   ├── providers/            # Price providers
│   │   ├── coingecko.ts      # CoinGecko API
│   │   └── binance.ts        # Binance API
│   ├── services/             # Core services
│   │   ├── price-validator.ts
│   │   ├── price-aggregator.ts
│   │   ├── cache.ts
│   │   └── contract-updater.ts
│   ├── types/                # TypeScript types
│   └── utils/                # Utilities
├── tests/                    # Test suites
└── package.json
```

## Cheers!
