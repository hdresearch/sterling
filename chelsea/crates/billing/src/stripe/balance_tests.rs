//! Tests for balance cache.

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::stripe::balance::{BalanceCache, CachedBalance};

    // ─── CachedBalance ──────────────────────────────────────────────

    #[test]
    fn effective_millicents_subtracts_pending() {
        let b = CachedBalance {
            available_cents: 1000,             // $10.00
            pending_spend_millicents: 500_000, // $5.00 in millicents
        };
        assert_eq!(b.effective_millicents(), 500_000); // $5.00 remaining
    }

    #[test]
    fn effective_millicents_can_go_negative() {
        let b = CachedBalance {
            available_cents: 100,
            pending_spend_millicents: 200_000,
        };
        assert_eq!(b.effective_millicents(), -100_000);
    }

    // ─── BalanceCache ────────────────────────────────────────────────

    #[tokio::test]
    async fn record_spend_updates_pending() {
        let cache = BalanceCache::new();
        let team_id = Uuid::new_v4();

        // Set initial balance
        cache.update(team_id, 1000).await;

        let b = cache.get(&team_id).await.unwrap();
        assert_eq!(b.available_cents, 1000);
        assert_eq!(b.pending_spend_millicents, 0);

        // Record spend
        cache.record_spend(&team_id, 150_000).await;
        let b = cache.get(&team_id).await.unwrap();
        assert_eq!(b.pending_spend_millicents, 150_000);
        assert_eq!(b.effective_millicents(), 850_000);

        // Record more spend
        cache.record_spend(&team_id, 50_000).await;
        let b = cache.get(&team_id).await.unwrap();
        assert_eq!(b.pending_spend_millicents, 200_000);
    }

    #[tokio::test]
    async fn update_resets_pending_spend() {
        let cache = BalanceCache::new();
        let team_id = Uuid::new_v4();

        cache.update(team_id, 1000).await;
        cache.record_spend(&team_id, 500_000).await;

        // Fresh poll resets pending
        cache.update(team_id, 500).await;
        let b = cache.get(&team_id).await.unwrap();
        assert_eq!(b.available_cents, 500);
        assert_eq!(b.pending_spend_millicents, 0);
    }

    #[tokio::test]
    async fn cache_miss_returns_none() {
        let cache = BalanceCache::new();
        assert!(cache.get(&Uuid::new_v4()).await.is_none());
    }

    #[tokio::test]
    async fn retain_teams_removes_stale() {
        let cache = BalanceCache::new();
        let keep = Uuid::new_v4();
        let remove = Uuid::new_v4();

        cache.update(keep, 100).await;
        cache.update(remove, 200).await;

        cache.retain_teams(&[keep]).await;

        assert!(cache.get(&keep).await.is_some());
        assert!(cache.get(&remove).await.is_none());
    }
}
