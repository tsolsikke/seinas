//! 描いたグリフの覚え(キャッシュ)。大きさに上限を置き、超えたら古く使ったものから捨てる(ADR 0003)。
//!
//! cosmic-text の `SwashCache` の覚えには上限が無いので、グリフの絵は `get_image_uncached` で作り、ここで覚える。

use std::collections::HashMap;

use cosmic_text::{CacheKey, SwashImage};

/// 既定の上限(バイト)。1 プロセスあたり。
pub const GLYPH_CACHE_LIMIT: usize = 4 * 1024 * 1024;
/// 1 つの項目の、絵のほかに掛かる分の見積もり(キー、位置、表の分)。
const ENTRY_OVERHEAD: usize = 96;

struct Entry {
    image: Option<SwashImage>,
    /// 最後に使った順番。
    used: u64,
    size: usize,
}

/// グリフの絵の覚え。
pub struct GlyphCache {
    entries: HashMap<CacheKey, Entry>,
    /// 覚えている分の大きさ(見積もり)と、上限。
    bytes: usize,
    limit: usize,
    clock: u64,
}

impl GlyphCache {
    pub fn new() -> Self {
        Self::with_limit(GLYPH_CACHE_LIMIT)
    }

    pub fn with_limit(limit: usize) -> Self {
        GlyphCache {
            entries: HashMap::new(),
            bytes: 0,
            limit,
            clock: 0,
        }
    }

    /// 覚えている分の大きさ(見積もり。バイト)。
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `key` のグリフの絵。覚えていなければ `make` で作って覚える(作れなかったことも覚える)。
    pub fn get_or_insert_with(
        &mut self,
        key: CacheKey,
        make: impl FnOnce() -> Option<SwashImage>,
    ) -> Option<&SwashImage> {
        self.clock += 1;
        let clock = self.clock;
        if !self.entries.contains_key(&key) {
            let image = make();
            let size = ENTRY_OVERHEAD + image.as_ref().map_or(0, |image| image.data.len());
            self.bytes += size;
            self.entries.insert(
                key,
                Entry {
                    image,
                    used: clock,
                    size,
                },
            );
            self.evict(key);
        }
        let entry = self.entries.get_mut(&key)?;
        entry.used = clock;
        entry.image.as_ref()
    }

    /// 上限を超えていたら、古く使ったものから捨てる(いま入れた `keep` は残す)。
    fn evict(&mut self, keep: CacheKey) {
        if self.bytes <= self.limit {
            return;
        }
        let mut order: Vec<(u64, CacheKey)> = self
            .entries
            .iter()
            .filter(|(key, _)| **key != keep)
            .map(|(key, entry)| (entry.used, *key))
            .collect();
        order.sort_unstable_by_key(|(used, _)| *used);
        // 1 つ超えるたびに並べ直さないよう、上限の 3/4 まで減らす。
        let target = self.limit / 4 * 3;
        for (_, key) in order {
            if self.bytes <= target {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.size;
            }
        }
    }
}

impl Default for GlyphCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use cosmic_text::{fontdb, CacheKeyFlags, SubpixelBin};

    use super::*;

    fn key(glyph: u16) -> CacheKey {
        CacheKey {
            font_id: fontdb::ID::dummy(),
            glyph_id: glyph,
            font_size_bits: 14f32.to_bits(),
            x_bin: SubpixelBin::Zero,
            y_bin: SubpixelBin::Zero,
            font_weight: fontdb::Weight::NORMAL,
            flags: CacheKeyFlags::empty(),
        }
    }

    fn image(bytes: usize) -> Option<SwashImage> {
        let mut image = SwashImage::new();
        image.data = vec![0; bytes];
        Some(image)
    }

    #[test]
    fn the_cache_keeps_under_its_limit_by_dropping_the_least_recently_used() {
        // 上限 4,000 バイト。1 つは 1,000 + 96 バイト。
        let mut cache = GlyphCache::with_limit(4_000);
        let mut made = 0;
        for glyph in 0..3 {
            cache.get_or_insert_with(key(glyph), || {
                made += 1;
                image(1_000)
            });
        }
        assert_eq!((cache.len(), made), (3, 3));
        // 覚えているものは作り直さない。0 を使うと、0 が新しくなる。
        cache.get_or_insert_with(key(0), || panic!("glyph 0 is cached"));
        // 4 つ目で上限を超えるので、上限の 3/4(3,000)まで、古く使ったもの(1、2)から捨てる。
        cache.get_or_insert_with(key(3), || image(1_000));
        assert!(cache.bytes() <= 3_000, "{}", cache.bytes());
        assert_eq!(cache.len(), 2);
        cache.get_or_insert_with(key(0), || panic!("glyph 0 was used recently"));
        cache.get_or_insert_with(key(3), || panic!("glyph 3 was just made"));
        let mut remade = false;
        cache.get_or_insert_with(key(1), || {
            remade = true;
            image(1_000)
        });
        assert!(remade, "glyph 1 was the least recently used");
    }

    #[test]
    fn a_glyph_without_an_image_is_remembered_too() {
        let mut cache = GlyphCache::new();
        assert!(cache.get_or_insert_with(key(7), || None).is_none());
        assert!(cache
            .get_or_insert_with(key(7), || panic!("remembered"))
            .is_none());
        assert_eq!(cache.bytes(), ENTRY_OVERHEAD);
    }
}
