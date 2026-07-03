// 一時的な再現テスト（後で削除）
#[cfg(test)]
mod repro_tests {
    use crate::geometry::*;
    use kurbo::Point;

    fn peg(x: f64, y: f64, r: f64) -> Peg {
        Peg { center: Point::new(x, y), radius: r }
    }

    /// 実マウスのように細かく補間しながらドラッグする
    fn drag(th: &mut Thread, pegs: &[Peg], waypoints: &[Point]) {
        let mut cur = th.tip;
        for &w in waypoints {
            let dist = cur.distance(w);
            let steps = (dist / 4.0).ceil().max(1.0) as usize;
            for i in 1..=steps {
                let t = i as f64 / steps as f64;
                let p = Point::new(cur.x + (w.x - cur.x) * t, cur.y + (w.y - cur.y) * t);
                update_thread(th, p, pegs);
            }
            cur = w;
        }
    }

    #[test]
    fn repro_two_peg_catch() {
        // アプリ実構成: spacing200, r=60, catch=6 → r_eff=66, グリッド(110,110)起点
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 110.0, 66.0)];
        // 開始 = ペグAの上の十字点
        let anchor = Point::new(110.0, 44.0);
        let mut th = Thread::new(anchor);
        drag(&mut th, &pegs, &[
            Point::new(250.0, 120.0),
            Point::new(420.0, 60.0),
            Point::new(480.0, 200.0),
        ]);
        eprintln!("wraps = {:?}", th.wraps);
        assert!(th.wraps.len() >= 2, "expected >=2 wraps, got {:?}", th.wraps);
    }

    #[test]
    fn repro_recatch_same_peg_over_top() {
        // A に巻いた後、B に巻き、その後 A の上をまた通る
        let pegs = [peg(110.0, 110.0, 66.0), peg(310.0, 110.0, 66.0)];
        let anchor = Point::new(110.0, 44.0);
        let mut th = Thread::new(anchor);
        drag(&mut th, &pegs, &[
            Point::new(180.0, 250.0),  // Aの右を下に → Aに巻く
            Point::new(310.0, 250.0),  // Bの下へ
            Point::new(420.0, 100.0),  // Bの右を上に → Bに巻く
            Point::new(310.0, 20.0),   // Bの上を左へ
            Point::new(110.0, 20.0),   // Aの上へ → Aに再度引っかかるはず
            Point::new(40.0, 200.0),   // Aの左を下に
        ]);
        eprintln!("wraps = {:?}", th.wraps);
        let peg0_wraps = th.wraps.iter().filter(|w| w.peg == 0).count();
        assert!(th.wraps.len() >= 3 && peg0_wraps >= 2,
            "expected A,B,A pattern, got {:?}", th.wraps);
    }
}
