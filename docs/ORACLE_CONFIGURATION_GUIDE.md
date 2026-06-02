# Oracle Configuration Management Guide

## Overview

This document outlines the procedures for managing oracle configurations in the StellarLend protocol, including role separation, security considerations, and operational guidelines.

## Architecture Overview

### Components

1. **Off-chain Oracle Service** (TypeScript/Node.js)
   - Fetches prices from multiple external sources
   - Aggregates and validates price data
   - Updates smart contract with validated prices via authorized `update_price_feed` operations

2. **Smart Contract Oracle Module** (Rust/Soroban)
   - Stores on-chain price feeds
   - Enforces validation rules and role separation
   - Manages oracle configuration, decimals calibration, and per-asset staleness boundaries

3. **Price Providers**
   - CoinGecko (primary, 60% weight)
   - Binance (secondary, 40% weight)
   - CoinMarketCap (optional, 35% weight)