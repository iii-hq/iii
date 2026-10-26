// SPDX-License-Identifier: Apache-2.0

//! A simple const generics substitute.

#[doc(hidden)]
#[macro_export]
macro_rules! impl_const_id {
    (
        $(#[$outer:meta])*
        $visibility:vis $trait:ident => $id_ty:ty;
        $(
            $iocty:ty = $val:expr
        ),* $(,)*
    ) => {
        $(#[$outer])*
        $visibility trait $trait {
            $(#[$outer])*
            const ID: $id_ty;
        }

        $(
            impl $trait for $iocty {
                const ID: $id_ty = $val;
            }
        )*
    };
}

#[cfg(test)]
mod tests {
    struct A;
    struct B;
    struct C;

    impl_const_id! {
        Id => usize;
        A = 1,
        B = 2,
        C = 3,
    }

    #[test]
    fn test_const_id_macro() {
        assert_eq!(A::ID, 1);
        assert_eq!(B::ID, 2);
        assert_eq!(C::ID, 3);
    }
}
