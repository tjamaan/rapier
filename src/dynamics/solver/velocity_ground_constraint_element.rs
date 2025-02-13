use super::DeltaVel;
use crate::math::{AngVector, Vector, DIM};
use crate::utils::{WBasis, WDot};
use na::SimdRealField;

#[derive(Copy, Clone, Debug)]
pub(crate) struct VelocityGroundConstraintTangentPart<N: SimdRealField + Copy> {
    pub gcross2: [AngVector<N>; DIM - 1],
    pub rhs: [N; DIM - 1],
    #[cfg(feature = "dim2")]
    pub impulse: na::Vector1<N>,
    #[cfg(feature = "dim3")]
    pub impulse: na::Vector2<N>,
    pub r: [N; DIM - 1],
}

impl<N: SimdRealField + Copy> VelocityGroundConstraintTangentPart<N> {
    #[cfg(any(not(target_arch = "wasm32"), feature = "simd-is-enabled"))]
    fn zero() -> Self {
        Self {
            gcross2: [na::zero(); DIM - 1],
            rhs: [na::zero(); DIM - 1],
            impulse: na::zero(),
            r: [na::zero(); DIM - 1],
        }
    }

    #[inline]
    pub fn solve(
        &mut self,
        tangents1: [&Vector<N>; DIM - 1],
        im2: N,
        limit: N,
        mj_lambda2: &mut DeltaVel<N>,
    ) where
        AngVector<N>: WDot<AngVector<N>, Result = N>,
        N::Element: SimdRealField + Copy,
    {
        #[cfg(feature = "dim2")]
        {
            let dimpulse = -tangents1[0].dot(&mj_lambda2.linear)
                + self.gcross2[0].gdot(mj_lambda2.angular)
                + self.rhs[0];
            let new_impulse = (self.impulse[0] - self.r[0] * dimpulse).simd_clamp(-limit, limit);
            let dlambda = new_impulse - self.impulse[0];
            self.impulse[0] = new_impulse;

            mj_lambda2.linear += tangents1[0] * (-im2 * dlambda);
            mj_lambda2.angular += self.gcross2[0] * dlambda;
        }

        #[cfg(feature = "dim3")]
        {
            let dimpulse_0 = -tangents1[0].dot(&mj_lambda2.linear)
                + self.gcross2[0].gdot(mj_lambda2.angular)
                + self.rhs[0];
            let dimpulse_1 = -tangents1[1].dot(&mj_lambda2.linear)
                + self.gcross2[1].gdot(mj_lambda2.angular)
                + self.rhs[1];

            let new_impulse = na::Vector2::new(
                self.impulse[0] - self.r[0] * dimpulse_0,
                self.impulse[1] - self.r[1] * dimpulse_1,
            );
            let new_impulse = {
                let _disable_fe_except =
                    crate::utils::DisableFloatingPointExceptionsFlags::
                    disable_floating_point_exceptions();
                new_impulse.simd_cap_magnitude(limit)
            };
            let dlambda = new_impulse - self.impulse;

            self.impulse = new_impulse;

            mj_lambda2.linear +=
                tangents1[0] * (-im2 * dlambda[0]) + tangents1[1] * (-im2 * dlambda[1]);
            mj_lambda2.angular += self.gcross2[0] * dlambda[0] + self.gcross2[1] * dlambda[1];
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct VelocityGroundConstraintNormalPart<N: SimdRealField + Copy> {
    pub gcross2: AngVector<N>,
    pub rhs: N,
    pub rhs_wo_bias: N,
    pub impulse: N,
    pub r: N,
}

impl<N: SimdRealField + Copy> VelocityGroundConstraintNormalPart<N> {
    #[cfg(any(not(target_arch = "wasm32"), feature = "simd-is-enabled"))]
    fn zero() -> Self {
        Self {
            gcross2: na::zero(),
            rhs: na::zero(),
            rhs_wo_bias: na::zero(),
            impulse: na::zero(),
            r: na::zero(),
        }
    }

    #[inline]
    pub fn solve(&mut self, dir1: &Vector<N>, im2: N, mj_lambda2: &mut DeltaVel<N>)
    where
        AngVector<N>: WDot<AngVector<N>, Result = N>,
    {
        let dimpulse =
            -dir1.dot(&mj_lambda2.linear) + self.gcross2.gdot(mj_lambda2.angular) + self.rhs;
        let new_impulse = (self.impulse - self.r * dimpulse).simd_max(N::zero());
        let dlambda = new_impulse - self.impulse;
        self.impulse = new_impulse;

        mj_lambda2.linear += dir1 * (-im2 * dlambda);
        mj_lambda2.angular += self.gcross2 * dlambda;
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct VelocityGroundConstraintElement<N: SimdRealField + Copy> {
    pub normal_part: VelocityGroundConstraintNormalPart<N>,
    pub tangent_part: VelocityGroundConstraintTangentPart<N>,
}

impl<N: SimdRealField + Copy> VelocityGroundConstraintElement<N> {
    #[cfg(any(not(target_arch = "wasm32"), feature = "simd-is-enabled"))]
    pub fn zero() -> Self {
        Self {
            normal_part: VelocityGroundConstraintNormalPart::zero(),
            tangent_part: VelocityGroundConstraintTangentPart::zero(),
        }
    }

    #[inline]
    pub fn solve_group(
        elements: &mut [Self],
        dir1: &Vector<N>,
        #[cfg(feature = "dim3")] tangent1: &Vector<N>,
        im2: N,
        limit: N,
        mj_lambda2: &mut DeltaVel<N>,
        solve_normal: bool,
        solve_friction: bool,
    ) where
        Vector<N>: WBasis,
        AngVector<N>: WDot<AngVector<N>, Result = N>,
        N::Element: SimdRealField + Copy,
    {
        // Solve penetration.
        if solve_normal {
            for element in elements.iter_mut() {
                element.normal_part.solve(&dir1, im2, mj_lambda2);
            }
        }

        // Solve friction.
        if solve_friction {
            #[cfg(feature = "dim3")]
            let tangents1 = [tangent1, &dir1.cross(&tangent1)];
            #[cfg(feature = "dim2")]
            let tangents1 = [&dir1.orthonormal_vector()];

            for element in elements.iter_mut() {
                let limit = limit * element.normal_part.impulse;
                let part = &mut element.tangent_part;
                part.solve(tangents1, im2, limit, mj_lambda2);
            }
        }
    }
}
