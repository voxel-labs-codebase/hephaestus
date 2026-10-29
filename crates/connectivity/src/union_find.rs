/// Disjoint sets with path halving and union by size.
pub(crate) struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    pub fn new(n: usize) -> Self {
        Self { parent: (0..n).collect(), size: vec![1; n] }
    }

    pub fn find(&mut self, mut x: usize) -> usize {
        while let Some(&p) = self.parent.get(x) {
            if p == x {
                break;
            }
            let grand = self.parent.get(p).copied().unwrap_or(p);
            if let Some(slot) = self.parent.get_mut(x) {
                *slot = grand;
            }
            x = grand;
        }
        x
    }

    pub fn union(&mut self, a: usize, b: usize) {
        let (mut a, mut b) = (self.find(a), self.find(b));
        if a == b {
            return;
        }
        if self.size.get(a) < self.size.get(b) {
            std::mem::swap(&mut a, &mut b);
        }
        if let Some(p) = self.parent.get_mut(b) {
            *p = a;
        }
        let sb = self.size.get(b).copied().unwrap_or(0);
        if let Some(sa) = self.size.get_mut(a) {
            *sa += sb;
        }
    }
}
