use serde::Serialize;

/// 1台分の、適用結果の累積。reset() されるまで fail・missed は減らない
/// （「失敗が次の成功で消える」ことを防ぐため）。
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub ok: u32,
    pub fail: u32,
    pub missed: u32,
    pub first_fail_seq: Option<u32>,
    #[serde(skip)]
    expected_next: u32,
}

impl Stats {
    pub fn new() -> Self {
        Self { ok: 0, fail: 0, missed: 0, first_fail_seq: None, expected_next: 1 }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 操作 seq の適用結果を積算する。期待していた連番より先に進んでいれば、
    /// その間に来るはずだった操作を取りこぼしとして数える。
    /// 連番が巻き戻っていたら（親機の再起動・数え直しで seq が 1 から振り直された場合）、
    /// 古い機器だけ累積が残ってずれて見えないよう、先にこちらもクリアする。
    pub fn record(&mut self, seq: u32, error: Option<&str>) {
        if seq < self.expected_next {
            self.reset();
        }
        if seq > self.expected_next {
            self.missed += seq - self.expected_next;
        }
        self.expected_next = seq + 1;
        if error.is_some() {
            self.fail += 1;
            self.first_fail_seq.get_or_insert(seq);
        } else {
            self.ok += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Stats;

    #[test]
    fn counts_consecutive_successes() {
        let mut s = Stats::new();
        s.record(1, None);
        s.record(2, None);
        s.record(3, None);
        assert_eq!(s.ok, 3);
        assert_eq!(s.fail, 0);
        assert_eq!(s.missed, 0);
    }

    #[test]
    fn detects_gap_as_missed() {
        let mut s = Stats::new();
        s.record(1, None);
        s.record(4, None); // 2, 3 を取りこぼした
        assert_eq!(s.missed, 2);
        assert_eq!(s.ok, 2);
    }

    #[test]
    fn failure_persists_through_later_success() {
        let mut s = Stats::new();
        s.record(1, Some("対象ウィンドウが見つかりません"));
        s.record(2, None);
        s.record(3, None);
        assert_eq!(s.fail, 1);
        assert_eq!(s.first_fail_seq, Some(1));
        assert_eq!(s.ok, 2);
    }

    #[test]
    fn first_fail_seq_keeps_earliest() {
        let mut s = Stats::new();
        s.record(1, Some("x"));
        s.record(2, Some("y"));
        assert_eq!(s.first_fail_seq, Some(1));
        assert_eq!(s.fail, 2);
    }

    #[test]
    fn reset_clears_everything_and_recounts_from_one() {
        let mut s = Stats::new();
        s.record(1, Some("x"));
        s.record(5, None);
        s.reset();
        assert_eq!(s.ok, 0);
        assert_eq!(s.fail, 0);
        assert_eq!(s.missed, 0);
        assert_eq!(s.first_fail_seq, None);
        s.record(1, None);
        assert_eq!(s.missed, 0);
        assert_eq!(s.ok, 1);
    }

    #[test]
    fn rewound_seq_clears_stale_accumulation() {
        // 親機の再起動・数え直しで、子機がこれまで積んだ分を抱えたまま
        // 新しい連番(1から)を受け取るケース。古い累積のまま加算され続けないこと。
        let mut s = Stats::new();
        s.record(1, None);
        s.record(2, None);
        s.record(3, Some("x"));
        assert_eq!(s.ok, 2);
        assert_eq!(s.fail, 1);

        s.record(1, None); // 親機側はseqを1から振り直した
        assert_eq!(s.ok, 1);
        assert_eq!(s.fail, 0);
        assert_eq!(s.missed, 0);
        assert_eq!(s.first_fail_seq, None);
    }
}
