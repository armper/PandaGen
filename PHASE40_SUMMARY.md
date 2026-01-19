# Phase 40: App Storefront Host (Curated Components, Policies, Budgets)

**Completion Date**: 2026-01-19

## Overview

Phase 40 introduces a **curated app storefront host** that enforces policies and budgets while resolving packages via the registry.

## What Was Added

### 1. Storefront Service (`services_app_store`)

- `AppListing` + `InstallPlan`
- `StorePolicy` trait
- `AllowAllPolicy` + `BudgetCapPolicy`

### 2. Registry Integration

- Uses `RegistryResolver` and build plans to resolve package entries

## Tests Added

- Install plan generation
- Policy denial on budget cap

## Files Changed

**New Files:**
- `services_app_store/Cargo.toml`
- `services_app_store/src/lib.rs`

**Modified Files:**
- `Cargo.toml` (workspace member + dependency)

## Conclusion

Phase 40 provides a policy-driven storefront layer for curated components and explicit budgets.
