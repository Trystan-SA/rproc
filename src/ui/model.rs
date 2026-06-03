//! Reconcile a persistent `VecModel` against a freshly built `Vec` without
//! replacing the model. Updating rows in place (`set_row_data`) keeps the
//! repeater's item identity stable across the 250 ms refresh tick — replacing
//! the whole model would recreate every delegate, dropping any click whose
//! press/release straddled a tick (the TouchArea that saw the press is gone by
//! release). Length changes push/remove only the delta.

use slint::{Model, VecModel};

pub fn sync<T: Clone + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    let old = model.row_count();
    let new = rows.len();
    for (i, row) in rows.into_iter().enumerate() {
        if i < old {
            model.set_row_data(i, row);
        } else {
            model.push(row);
        }
    }
    for _ in new..old {
        model.remove(model.row_count() - 1);
    }
}
