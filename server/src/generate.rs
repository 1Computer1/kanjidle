use std::time::Instant;

use itertools::Itertools;
use ordered_float::OrderedFloat;

use crate::data::{Ji, KanjiClass, KanjiData, Loc, WordData, MAX_KANJI_RANK, MAX_WORD_RANK};

#[derive(Debug)]
pub struct PuzzleOptions {
    // Kanji picking options
    pub min_kanji_class: KanjiClass,
    pub max_kanji_class: KanjiClass,
    pub rare_kanji_bias: f64,
    pub fixed: Option<Ji>,

    // Hint picking options
    pub min_word_rarity: usize,
    pub max_word_rarity: usize,
    pub min_word_kanji_class: KanjiClass, // Applies to words as well
    pub max_word_kanji_class: KanjiClass, // Applies to words as well

    // Hint ordering options
    pub irregular_hint_bias: f64,
    pub rare_kanji_hint_bias: f64,
    pub rare_word_hint_bias: f64,

    // Puzzle size options
    pub num_hints: usize,
    pub guarantee_answer_by: usize,
}

#[derive(Debug, Clone)]
pub struct Hint {
    pub answer: Ji,
    pub answer_location: Loc,
    pub hint: Ji,
    pub rank: usize,
    pub irregularness: f64,
}

#[derive(Debug)]
pub struct Puzzle {
    pub answer: Ji,
    pub hints: Vec<Hint>,
    pub extra_hints: Vec<Hint>,
}

#[derive(Debug)]
pub struct Generator<'a, R> {
    pub rng: R,
    pub kanji_data: &'a KanjiData,
    pub word_data: &'a WordData,
}

impl<'a, R> Generator<'a, R> {
    pub fn new(rng: R, kanji_data: &'a KanjiData, word_data: &'a WordData) -> Generator<'a, R> {
        Generator {
            rng,
            kanji_data,
            word_data,
        }
    }
}

impl<'g, R: rand::Rng> Generator<'g, R> {
    pub fn choose_puzzle(&'g mut self, options: &PuzzleOptions) -> Puzzle {
        let start = Instant::now();
        let mut ks = self.choose_kanji(options);
        if let Some(fixed) = options.fixed {
            ks.insert(0, fixed);
        }
        let t = start.elapsed();
        tracing::debug!("Chose kanjis in {t:?}");

        let start1 = Instant::now();
        for answer in ks {
            let start = Instant::now();
            let hints = self.find_usable_hints(answer, options);
            let puzzle: Option<Puzzle> = self.choose_puzzle_from(answer, &hints, options);
            let t = start.elapsed();
            tracing::debug!("Checked hints in {t:?}");

            if let Some(p) = puzzle {
                let t = start1.elapsed();
                tracing::debug!("Chose final puzzle in {t:?}");
                return p;
            }
        }
        panic!("Could not generate a puzzle, this should not occur")
    }

    pub fn choose_kanji(&mut self, options: &PuzzleOptions) -> Vec<Ji> {
        let ks = self
            .kanji_data
            .kanjis
            .values()
            .filter(|k| {
                let nums = "一ニ三四五六七八九十百千万";
                !nums.contains(k.ji.0)
            })
            .filter(|k| {
                let class = self.kanji_data.kanji_metas.get(&k.ji).unwrap().class;
                options.min_kanji_class <= class && class <= options.max_kanji_class
            })
            .collect_vec();
        weighted_shuffle(&ks, &mut self.rng, |k| {
            apply_bias(
                options.rare_kanji_bias,
                k.rank as f64 / MAX_KANJI_RANK as f64,
            )
        })
        .map(|k| k.ji)
        .collect()
    }

    pub fn find_usable_hints(&self, answer: Ji, options: &PuzzleOptions) -> Vec<Hint> {
        self.word_data
            .twos
            .values()
            .skip_while(|x| x.word.rank <= options.min_word_rarity)
            .take_while(|x| x.word.rank <= options.max_word_rarity)
            .filter(|two| {
                let class_a = self.kanji_data.kanji_metas.get(&two.a).unwrap().class;
                let class_b = self.kanji_data.kanji_metas.get(&two.b).unwrap().class;
                options.min_word_kanji_class <= class_a
                    && class_a <= options.max_word_kanji_class
                    && options.min_word_kanji_class <= class_b
                    && class_b <= options.max_word_kanji_class
            })
            .filter_map(|two| {
                let (hint, answer, answer_location) = if two.a == answer {
                    (two.b, two.a, Loc::L)
                } else if two.b == answer {
                    (two.a, two.b, Loc::R)
                } else {
                    return None;
                };
                Some(Hint {
                    hint,
                    answer_location,
                    answer,
                    irregularness: two.irregularness,
                    rank: two.word.rank,
                })
            })
            .collect()
    }

    pub fn choose_puzzle_from(
        &mut self,
        answer: Ji,
        hints: &[Hint],
        options: &PuzzleOptions,
    ) -> Option<Puzzle> {
        if hints.len() < options.num_hints {
            return None;
        }

        let mut splits: Vec<Vec<_>> = split(&hints.iter().rev().collect_vec(), options.num_hints)
            .map(|s| s.to_vec())
            .collect();
        for split in &mut splits {
            *split = weighted_shuffle(split, &mut self.rng, |x| {
                let b1 = apply_bias(options.irregular_hint_bias, x.irregularness);

                let kanji_rank = self.kanji_data.kanjis.get(&x.hint).unwrap().rank;

                let b2 = apply_bias(
                    options.rare_kanji_hint_bias,
                    kanji_rank as f64 / MAX_KANJI_RANK as f64,
                );

                let b3 = apply_bias(
                    options.rare_word_hint_bias,
                    x.rank as f64 / MAX_WORD_RANK as f64,
                );

                b1 * b2 * b3
            })
            .copied()
            .collect();
        }
        let splits = splits;

        let guaranteed_hints = &splits[0..options.guarantee_answer_by];
        for ixes in
            multi_cartesian_diagonal(&guaranteed_hints.iter().map(|s| s.len()).collect_vec())
        {
            let chosen_hints = guaranteed_hints
                .iter()
                .zip(ixes)
                .map(|(s, ix)| s[ix])
                .collect_vec();
            let extra_hints: Vec<&Hint> = splits[options.guarantee_answer_by..options.num_hints]
                .iter()
                .map(|s| s[0])
                .collect();
            let all_hints = [&chosen_hints[..], &extra_hints[..]].concat();

            if !self.contains_same_hint(&all_hints)
                && !self.contains_same_numerical_hint(&all_hints)
                && self
                    .find_unintended_solutions(answer, &chosen_hints)
                    .is_empty()
            {
                return Some(Puzzle {
                    answer,
                    hints: chosen_hints.iter().copied().cloned().collect(),
                    extra_hints: extra_hints.iter().copied().cloned().collect(),
                });
            }
        }

        None
    }

    pub fn contains_same_hint(&self, hints: &[&Hint]) -> bool {
        hints.iter().combinations(2).any(|hs| {
            let variants = &self
                .kanji_data
                .kanji_metas
                .get(&hs[0].hint)
                .unwrap()
                .variants;
            variants.iter().any(|v| v == &hs[1].hint)
        })
    }

    pub fn contains_same_numerical_hint(&self, hints: &[&Hint]) -> bool {
        let nums = "一ニ三四五六七八九十百千万";
        hints.iter().combinations(2).any(|hs| {
            if nums.contains(hs[0].answer.0) {
                // Stricter check if the answer is a number
                nums.contains(hs[0].hint.0) && nums.contains(hs[1].hint.0)
            } else {
                hs[0].answer_location == hs[1].answer_location
                    && nums.contains(hs[0].hint.0)
                    && nums.contains(hs[1].hint.0)
            }
        })
    }

    pub fn find_unintended_solutions(&self, answer: Ji, hints: &[&Hint]) -> Vec<Ji> {
        self.kanji_data
            .kanjis
            .keys()
            .copied()
            .filter(|&k| {
                k != answer
                    && hints.iter().all(|x| match x.answer_location {
                        Loc::L => {
                            let key = String::from_iter(&[k.0, x.hint.0]);
                            self.word_data.twos.contains_key(&key)
                        }
                        Loc::R => {
                            let key = String::from_iter(&[x.hint.0, k.0]);
                            self.word_data.twos.contains_key(&key)
                        }
                    })
            })
            .collect()
    }
}

fn apply_bias(bias: f64, x: f64) -> f64 {
    1.0 + (bias - 1.0) * x
}

// https://users.rust-lang.org/t/how-to-split-a-slice-into-n-chunks/40008/3
fn split<T>(slice: &[T], n: usize) -> impl Iterator<Item = &[T]> {
    struct Split<'a, T> {
        slice: &'a [T],
        len: usize,
        rem: usize,
    }

    impl<'a, T> Iterator for Split<'a, T> {
        type Item = &'a [T];

        fn next(&mut self) -> Option<Self::Item> {
            if self.slice.is_empty() {
                return None;
            }
            let mut len = self.len;
            if self.rem > 0 {
                len += 1;
                self.rem -= 1;
            }
            let (chunk, rest) = self.slice.split_at(len);
            self.slice = rest;
            Some(chunk)
        }
    }
    let len = slice.len() / n;
    let rem = slice.len() % n;
    Split { slice, len, rem }
}

fn weighted_shuffle<'a, T, R: rand::Rng, F: Fn(&T) -> f64>(
    items: &'a [T],
    rng: &mut R,
    weight: F,
) -> impl Iterator<Item = &'a T> {
    (0..items.len())
        .map(|i| (i, -rng.gen::<f64>().powf(1.0 / weight(&items[i]))))
        .sorted_by_key(|x| OrderedFloat(x.1))
        .map(|x| &items[x.0])
}

// Iterate through the indices of an n-dimensional matrix in diagonal order.
fn multi_cartesian_diagonal(dims: &[usize]) -> impl Iterator<Item = Vec<usize>> + use<'_> {
    fn step<'b>(i: usize, dims: &'b [usize]) -> Box<dyn Iterator<Item = Vec<usize>> + 'b> {
        if dims.len() == 1 {
            if i < dims[0] {
                Box::new(std::iter::once(vec![i]))
            } else {
                Box::new(std::iter::empty())
            }
        } else {
            Box::new(
                (0..=i).flat_map(move |j| -> Box<dyn Iterator<Item = Vec<usize>> + 'b> {
                    let i = i - j;
                    if i < dims[0] {
                        Box::new(step(j, &dims[1..]).map(move |mut ix| {
                            let mut a = vec![i];
                            a.append(&mut ix);
                            a
                        }))
                    } else {
                        Box::new(std::iter::empty())
                    }
                }),
            )
        }
    }

    (0..=dims.iter().sum()).flat_map(|i| step(i, dims))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffle_obvious() {
        let out = weighted_shuffle(&[100, 0], &mut rand::thread_rng(), |x| *x as f64);
        assert_eq!(out.collect_vec(), vec![&100, &0]);
    }

    fn cartesian_diagonal(n1: usize, n2: usize) -> impl Iterator<Item = Vec<usize>> {
        (0..=n1 + n2).flat_map(move |i| {
            (0..=i).filter_map(move |j| {
                let i = i - j;
                if i < n1 && j < n2 {
                    Some(vec![i, j])
                } else {
                    None
                }
            })
        })
    }

    #[test]
    fn multi_cartesian_diagonal_2d() {
        let ixes1 = cartesian_diagonal(3, 4).collect_vec();
        assert_eq!(ixes1.len(), 3 * 4);
        assert!(ixes1
            .iter()
            .tuple_windows()
            .all(|(a, b)| a.iter().sum::<usize>() <= b.iter().sum()));
        assert_eq!(
            ixes1.iter().map(|x| x.iter().sum()).counts(),
            [(0, 1), (1, 2), (2, 3), (3, 3), (4, 2), (5, 1)].into()
        );

        let ixes2 = multi_cartesian_diagonal(&[3, 4]).collect_vec();
        assert_eq!(ixes1, ixes2);
    }

    fn cartesian_diagonal_3d(n1: usize, n2: usize, n3: usize) -> impl Iterator<Item = Vec<usize>> {
        (0..=n1 + n2 + n3).flat_map(move |i| {
            (0..=i).flat_map(move |j| {
                let i = i - j;
                (0..=j).filter_map(move |k| {
                    let j = j - k;
                    if i < n1 && j < n2 && k < n3 {
                        Some(vec![i, j, k])
                    } else {
                        None
                    }
                })
            })
        })
    }

    #[test]
    fn multi_cartesian_diagonal_3d() {
        let ixes1 = cartesian_diagonal_3d(3, 4, 5).collect_vec();
        assert_eq!(ixes1.len(), 3 * 4 * 5);
        assert!(ixes1
            .iter()
            .tuple_windows()
            .all(|(a, b)| a.iter().sum::<usize>() <= b.iter().sum()));
        assert_eq!(
            ixes1.iter().map(|x| x.iter().sum()).counts(),
            [
                (0, 1),
                (1, 3),
                (2, 6),
                (3, 9),
                (4, 11),
                (5, 11),
                (6, 9),
                (7, 6),
                (8, 3),
                (9, 1)
            ]
            .into()
        );

        let ixes2 = multi_cartesian_diagonal(&[3, 4, 5]).collect_vec();
        assert_eq!(ixes1, ixes2);
    }

    #[test]
    #[allow(clippy::identity_op)]
    fn multi_cartesian_diagonal_nd() {
        let ixes = multi_cartesian_diagonal(&[3, 4, 5, 2, 1, 10]).collect_vec();
        assert_eq!(ixes.len(), 3 * 4 * 5 * 2 * 1 * 10);
        assert!(ixes
            .iter()
            .tuple_windows()
            .all(|(a, b)| a.iter().sum::<usize>() <= b.iter().sum()));
    }
}
