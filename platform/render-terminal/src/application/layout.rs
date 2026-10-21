/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

use super::CellRect;
use super::scene::{Direction, Layout};

pub(super) struct Flow {
    pub size: (u16, u32),
    pub children: Vec<CellRect>,
}
fn basis(layout: Layout, available: Option<u32>, intrinsic: u32) -> u32 {
    if let Some(value) = layout.basis {
        return u32::from(value);
    }
    if let Some(fraction) = layout.basis_fraction {
        let value = available.map_or(intrinsic, |available| {
            (u64::from(available) * u64::from(fraction.numerator) / u64::from(fraction.denominator))
                as u32
        });
        return value
            .max(u32::from(fraction.min))
            .min(fraction.max.map_or(u32::MAX, u32::from));
    }
    intrinsic
}
/// Row width allocation precedes cross-axis measurement. Narrow basis/grow
/// children and wrapped rows must report their height at the actual width.
pub(super) fn measured<E>(
    direction: Direction,
    width: u16,
    height: Option<u32>,
    gap: u16,
    wrap: bool,
    layouts: &[Layout],
    mut measure: impl FnMut(usize, u16) -> Result<(u16, u32), E>,
) -> Result<Flow, E> {
    let mut children = layouts
        .iter()
        .enumerate()
        .map(|(index, &layout)| measure(index, width).map(|size| (size, layout)))
        .collect::<Result<Vec<_>, _>>()?;
    let allocated = flow(direction, width, height, gap, wrap, &children);
    if matches!(direction, Direction::Row) {
        for (index, rect) in allocated.children.iter().enumerate() {
            if rect.width != width {
                children[index].0.1 = measure(index, rect.width)?.1;
            }
        }
        Ok(flow(direction, width, height, gap, wrap, &children))
    } else {
        Ok(allocated)
    }
}
/// One cell-allocation algorithm for measurement and paint. An unbounded
/// column measures intrinsic rows; a bounded viewport distributes grow space.
pub(super) fn flow(
    direction: Direction,
    width: u16,
    height: Option<u32>,
    gap: u16,
    wrap: bool,
    children: &[((u16, u32), Layout)],
) -> Flow {
    if matches!(direction, Direction::Row) && wrap {
        let mut rows: Vec<Vec<((u16, u32), Layout)>> = Vec::new();
        let mut occupied = 0u32;
        for &(size, mut layout) in children {
            let basis = basis(layout, Some(u32::from(width)), u32::from(size.0))
                .min(u32::from(width)) as u16;
            if rows.last().is_some_and(|row| !row.is_empty())
                && occupied + u32::from(gap) + u32::from(basis) > u32::from(width)
            {
                occupied = 0;
                rows.push(Vec::new());
            }
            if rows.is_empty() {
                rows.push(Vec::new());
            }
            let row = rows.last_mut().unwrap();
            if !row.is_empty() {
                occupied += u32::from(gap);
            }
            occupied += u32::from(basis);
            layout.basis = Some(basis);
            layout.basis_fraction = None;
            row.push((size, layout));
        }
        let (mut y, mut widest) = (0u32, 0u16);
        let mut placed = Vec::with_capacity(children.len());
        for (index, row) in rows.iter().enumerate() {
            let line = flow(Direction::Row, width, None, gap, false, row);
            widest = widest.max(line.size.0);
            for mut rect in line.children {
                rect.y = i64::from(y);
                placed.push(rect);
            }
            y = y.saturating_add(line.size.1);
            if index + 1 < rows.len() {
                y = y.saturating_add(u32::from(gap));
            }
        }
        return Flow {
            size: (widest, y),
            children: placed,
        };
    }
    let horizontal = matches!(direction, Direction::Row);
    let available = if horizontal {
        Some(u32::from(width))
    } else {
        height
    };
    let bases: Vec<u32> = children
        .iter()
        .map(|&(size, layout)| {
            basis(
                layout,
                available,
                if layout.grow > 0 && available.is_some() {
                    0
                } else if horizontal {
                    u32::from(size.0)
                } else {
                    size.1
                },
            )
        })
        .collect();
    let spacing = u32::from(gap)
        .saturating_mul(u32::try_from(children.len().saturating_sub(1)).unwrap_or(u32::MAX));
    let occupied = bases
        .iter()
        .copied()
        .fold(0u32, u32::saturating_add)
        .saturating_add(spacing);
    let free = available.map_or(0, |n| n.saturating_sub(occupied));
    let shrinkable: u64 = children
        .iter()
        .zip(&bases)
        .filter(|((_, layout), _)| layout.shrink)
        .map(|(_, base)| u64::from(*base))
        .sum();
    let deficit = u64::from(available.map_or(0, |n| occupied.saturating_sub(n))).min(shrinkable);
    // With no shrinkable basis both cumulative numerators remain zero.
    let shrinkable = shrinkable.max(1);
    let weight = children
        .iter()
        .map(|(_, layout)| u64::from(layout.grow))
        .sum::<u64>();
    let (mut offset, mut cross, mut used_weight) = (0u32, 0u32, 0u64);
    let mut used_shrink = 0u64;
    let mut placed = Vec::with_capacity(children.len());
    for (&(size, layout), base) in children.iter().zip(bases) {
        let before = used_weight;
        used_weight += u64::from(layout.grow);
        let extra = if weight == 0 {
            0
        } else {
            (u128::from(free) * u128::from(used_weight) / u128::from(weight)
                - u128::from(free) * u128::from(before) / u128::from(weight)) as u32
        };
        let before_shrink = used_shrink;
        if layout.shrink {
            used_shrink += u64::from(base);
        }
        let reduction = (u128::from(deficit) * u128::from(used_shrink) / u128::from(shrinkable)
            - u128::from(deficit) * u128::from(before_shrink) / u128::from(shrinkable))
            as u32;
        let length = base.saturating_sub(reduction).saturating_add(extra);
        let length = if horizontal {
            length.min(u32::from(width).saturating_sub(offset))
        } else {
            length
        };
        placed.push(if horizontal {
            CellRect::new(
                i64::from(offset),
                0,
                length as u16,
                height.unwrap_or(size.1),
            )
        } else {
            CellRect::new(0, i64::from(offset), width, length)
        });
        cross = cross.max(if horizontal {
            size.1
        } else {
            u32::from(size.0)
        });
        offset = offset.saturating_add(length);
        if placed.len() < children.len() {
            offset = offset.saturating_add(u32::from(gap));
        }
    }
    let length = offset;
    Flow {
        size: if horizontal {
            (length.min(u32::from(width)) as u16, cross)
        } else {
            (cross.min(u32::from(width)) as u16, length)
        },
        children: placed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fractional_bases_use_the_allocated_axis_and_clamp_before_growth() {
        let list = Layout {
            basis_fraction: Some(maka_ui_runtime::scene::terminal::FractionalBasis {
                numerator: 1,
                denominator: 3,
                min: 1,
                max: Some(6),
            }),
            ..Default::default()
        };
        let children = [
            ((50, 100), list),
            (
                (1, 1),
                Layout {
                    basis: Some(1),
                    ..Default::default()
                },
            ),
            (
                (50, 80),
                Layout {
                    grow: 1,
                    basis: Some(0),
                    ..Default::default()
                },
            ),
        ];
        for height in 2..40 {
            let flow = flow(Direction::Column, 50, Some(height), 0, false, &children);
            assert_eq!(flow.children[0].height, (height / 3).clamp(1, 6));
            assert_eq!(
                flow.children[2].height,
                height - flow.children[0].height - 1
            );
        }
        assert_eq!(
            flow(Direction::Column, 50, None, 0, false, &children).children[0].height,
            6
        );
        let mut wide = children;
        wide[0].1.basis_fraction = Some(maka_ui_runtime::scene::terminal::FractionalBasis {
            numerator: 34,
            denominator: 100,
            min: 0,
            max: None,
        });
        for width in 100..200 {
            let flow = flow(Direction::Row, width, Some(24), 0, false, &wide);
            assert_eq!(flow.children[0].width, width * 34 / 100);
            assert_eq!(flow.children[2].width, width - flow.children[0].width - 1);
        }
        let halves = [(
            (90, 1),
            Layout {
                basis_fraction: Some(maka_ui_runtime::scene::terminal::FractionalBasis {
                    numerator: 1,
                    denominator: 2,
                    min: 0,
                    max: None,
                }),
                ..Default::default()
            },
        ); 2];
        let wrapped = flow(Direction::Row, 100, None, 1, true, &halves);
        assert_eq!(
            wrapped.children,
            [CellRect::new(0, 0, 50, 1), CellRect::new(0, 2, 50, 1)]
        );
    }
    #[test]
    fn shrinking_keeps_fixed_controls_and_distributes_every_cell() {
        let shrink = Layout {
            shrink: true,
            ..Default::default()
        };
        let children = [
            ((8, 8), shrink),
            ((4, 4), shrink),
            ((3, 3), Layout::default()),
        ];
        for available in 0..20 {
            let column = flow(Direction::Column, 20, Some(available), 1, false, &children);
            let row = flow(Direction::Row, available as u16, None, 1, false, &children);
            assert_eq!(column.children[2].height, 3);
            assert_eq!(column.size.1, available.clamp(5, 17));
            if available >= 5 {
                assert_eq!(row.size.0, column.size.1 as u16);
                for (a, b) in row.children.iter().zip(&column.children) {
                    assert_eq!(u32::from(a.width), b.height);
                }
            }
        }
        // Measuring unbounded content and undeclared children keeps the
        // original intrinsic sizes, including overflow in scroll containers.
        assert_eq!(
            flow(Direction::Column, 20, None, 1, false, &children)
                .size
                .1,
            17
        );
        let fixed = [((8, 8), Layout::default()), ((3, 3), Layout::default())];
        assert_eq!(
            flow(Direction::Column, 20, Some(5), 1, false, &fixed)
                .size
                .1,
            12
        );
    }
    #[test]
    fn fixed_siblings_reserve_cells_before_long_flexible_content() {
        let children = [
            ((20, 2), Layout::default()),
            (
                (20, 8192),
                Layout {
                    grow: 1,
                    ..Layout::default()
                },
            ),
            ((20, 3), Layout::default()),
        ];
        let layout = flow(Direction::Column, 20, Some(12), 1, false, &children);
        assert_eq!(
            layout.children,
            [
                CellRect::new(0, 0, 20, 2),
                CellRect::new(0, 3, 20, 5),
                CellRect::new(0, 9, 20, 3)
            ]
        );
        assert_eq!(
            flow(Direction::Column, 20, None, 1, false, &children)
                .size
                .1,
            8199
        );
    }
    #[test]
    fn weighted_growth_uses_every_remaining_cell_once() {
        let children = [
            (
                (1, 9),
                Layout {
                    grow: 1,
                    ..Layout::default()
                },
            ),
            (
                (1, 9),
                Layout {
                    grow: 2,
                    ..Layout::default()
                },
            ),
            ((1, 2), Layout::default()),
        ];
        let layout = flow(Direction::Column, 7, Some(10), 0, false, &children);
        assert_eq!(
            layout.children.iter().map(|r| r.height).collect::<Vec<_>>(),
            [2, 6, 2]
        );
        let layout = flow(Direction::Row, 10, Some(3), 0, false, &children);
        assert_eq!(
            layout.children.iter().map(|r| r.width).collect::<Vec<_>>(),
            [3, 6, 1]
        );
    }
    #[test]
    fn wrapping_preserves_control_order_and_reports_actual_rows() {
        let children = [
            ((8, 1), Layout::default()),
            ((9, 1), Layout::default()),
            ((6, 1), Layout::default()),
        ];
        let layout = flow(Direction::Row, 16, None, 1, true, &children);
        assert_eq!(
            layout.children,
            [
                CellRect::new(0, 0, 8, 1),
                CellRect::new(0, 2, 9, 1),
                CellRect::new(10, 2, 6, 1)
            ]
        );
        assert_eq!(layout.size, (16, 3));
    }
}

#[cfg(test)]
mod wrap_growth_tests {
    use super::*;
    #[test]
    fn wrapped_rows_distribute_remaining_width_inside_each_row() {
        let children = [
            (
                (8, 1),
                Layout {
                    grow: 1,
                    ..Layout::default()
                },
            ),
            ((8, 1), Layout::default()),
            (
                (6, 1),
                Layout {
                    grow: 2,
                    ..Layout::default()
                },
            ),
        ];
        let layout = flow(Direction::Row, 18, None, 1, true, &children);
        assert_eq!(
            layout.children,
            [
                CellRect::new(0, 0, 9, 1),
                CellRect::new(10, 0, 8, 1),
                CellRect::new(0, 2, 18, 1)
            ]
        );
        assert_eq!(layout.size, (18, 3));
    }
}
