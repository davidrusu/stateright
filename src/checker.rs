//! Private module for selective re-export.

mod bfs;
use crate::{fingerprint, Fingerprint, Expectation, Model};
mod dfs;
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;

pub use bfs::*;
pub use dfs::*;

/// A path of states including actions. i.e. `state --action--> state ... --action--> state`.
/// You can convert to a `Vec<_>` with [`path.into_vec()`]. If you only need the actions, then use
/// [`path.into_actions()`].
///
/// [`path.into_vec()`]: Path::into_vec
/// [`path.into_actions()`]: Path::into_actions
#[derive(Clone, Debug, PartialEq)]
pub struct Path<State, Action>(pub Vec<(State, Option<Action>)>);
impl<State, Action> Path<State, Action> {
    /// Constructs a path from a model and a sequence of fingerprints.
    fn from_model_and_fingerprints<M>(model: &M, mut fingerprints: VecDeque<Fingerprint>) -> Path<M::State, M::Action>
    where M: Model<State = State, Action = Action>,
          M::State: Hash,
    {
        let init_print = match fingerprints.pop_front() {
            Some(init_print) => init_print,
            None => panic!("empty path is invalid"),
        };
        let mut last_state = model.init_states().into_iter()
            .find(|s| {
                fingerprint(&s) == init_print
            })
            .expect("no init state matches fingerprint");
        let mut output = Vec::new();
        while let Some(next_fp) = fingerprints.pop_front() {
            let mut actions = Vec::new();
            model.actions(
                &last_state,
                &mut actions);

            let (action, next_state) = model
                .next_steps(&last_state).into_iter()
                .find_map(|(a,s)| {
                    println!("{} == {}", fingerprint(&s), next_fp);
                    if fingerprint(&s) == next_fp {
                        println!("match");
                        Some((a, s))
                    } else {
                        println!("no match");
                        None
                    }
                }).expect("no next state matches fingerprint");
            output.push((last_state, Some(action)));

            last_state = next_state;
        }
        output.push((last_state, None));
        Path(output)
    }

    /// Determines the final state associated with a particular fingerprint path.
    pub(crate) fn final_state<M>(model: &M, mut fingerprints: VecDeque<Fingerprint>) -> Option<M::State>
    where M: Model<State = State, Action = Action>,
          M::State: Hash,
    {
        let init_print = match fingerprints.pop_front() {
            Some(init_print) => init_print,
            None => return None,
        };
        let mut matching_state =
            match model.init_states().into_iter().find(|s| fingerprint(&s) == init_print) {
                Some(matching_state) => matching_state,
                None => return None,
            };
        while let Some(next_print) = fingerprints.pop_front() {
            matching_state =
                match model.next_states(&matching_state).into_iter().find(|s| fingerprint(&s) == next_print) {
                    Some(matching_state) => matching_state,
                    None => return None,
                };
        }
        Some(matching_state)
    }

    /// Extracts the last state.
    pub fn last_state(&self) -> &State {
        &self.0.last().unwrap().0
    }

    /// Extracts the actions.
    pub fn into_actions(self) -> Vec<Action> {
        self.0.into_iter().filter_map(|(_s, a)| a).collect()
    }

    /// Convenience method for `Into<Vec<_>>`.
    pub fn into_vec(self) -> Vec<(State, Option<Action>)> {
        self.into()
    }

    /// Encodes the path as a sequence of opaque "fingerprints" delimited by forward
    /// slash (`/`) characters.
    pub fn encode(&self) -> String where State: Hash {
        self.0.iter()
            .map(|(s, _a)| format!("{}", fingerprint(s)))
            .collect::<Vec<String>>()
            .join("/")
    }
}
impl<State, Action> Into<Vec<(State, Option<Action>)>> for Path<State, Action> {
    fn into(self) -> Vec<(State, Option<Action>)> { self.0 }
}

/// A trait providing convenience methods for [`Model`] checkers. Implemented by
/// [`BfsChecker`] and [`DfsChecker`].
pub trait ModelChecker<M: Model> {
    /// Returns a reference to this checker's [`Model`].
    fn model(&self) -> &M;

    /// Indicates how many states have been generated.
    fn generated_count(&self) -> usize;

    /// Indicates how many generated states are pending verification.
    fn pending_count(&self) -> usize;

    /// Returns a map from property name to corresponding "discovery" (indicated
    /// by a [`Path`]).
    fn discoveries(&self) -> HashMap<&'static str, Path<M::State, M::Action>>;

    /// Visits up to a specified number of states checking the model's properties. May return
    /// earlier when all states have been checked or all the properties are resolved.
    fn check(&mut self, max_count: usize) -> &mut Self;

    /// Indicates that either all properties have associated discoveries or all reachable states
    /// have been visited.
    fn is_done(&self) -> bool {
        self.discoveries().len() == self.model().properties().len()
            || self.pending_count() == 0
    }

    /// An example of a "sometimes" property. `None` indicates that the property exists but no
    /// example has been found. Will panic if a corresponding "sometimes" property does not
    /// exist.
    fn example(&self, name: &'static str) -> Option<Path<M::State, M::Action>> {
        if let Some(p) = self.model().properties().into_iter().find(|p| p.name == name) {
            if p.expectation != Expectation::Sometimes {
                panic!("Please use `counterexample(\"{}\")` for this `always` or `eventually` property.", name);
            }
            self.discoveries().remove(name)
        } else {
            let available: Vec<_> = self.model().properties().iter().map(|p| p.name).collect();
            panic!("Unknown property. requested={:?}, available={:?}", name, available);
        }
    }

    /// A counterexample of an "always" or "eventually" property. `None` indicates that the property exists but no
    /// counterexample has been found. Will panic if a corresponding "always" or "eventually" property does not
    /// exist.
    fn counterexample(&self, name: &'static str) -> Option<Path<M::State, M::Action>> {
        if let Some(p) = self.model().properties().iter().find(|p| p.name == name) {
            if p.expectation == Expectation::Sometimes {
                panic!("Please use `example(\"{}\")` for this `sometimes` property.", name);
            }
            self.discoveries().remove(name)
        } else {
            let available: Vec<_> = self.model().properties().iter().map(|p| p.name).collect();
            panic!("Unknown property. requested={}, available={:?}", name, available);
        }
    }

    /// Blocks the thread until model checking is complete. Periodically emits a status while
    /// checking, tailoring the block size to the checking speed. Emits a report when complete.
    fn check_and_report(&mut self, w: &mut impl std::io::Write)
    where M::Action: Debug,
          M::State: Debug,
    {
        use std::cmp::max;
        use std::time::Instant;

        let method_start = Instant::now();
        let mut block_size = 32_768;
        loop {
            let block_start = Instant::now();
            if self.check(block_size).is_done() {
                let elapsed = method_start.elapsed().as_secs();
                for (name, path) in self.discoveries() {
                    writeln!(w, "== {} ==", name).unwrap();
                    for action in path.into_actions() {
                        writeln!(w, "ACTION: {:?}", action).unwrap();
                    }
                }
                writeln!(w, "Complete. generated={}, pending={}, sec={}",
                    self.generated_count(),
                    self.pending_count(),
                    elapsed
                ).unwrap();
                return;
            }

            let block_elapsed = block_start.elapsed().as_secs();
            if block_elapsed > 0 {
                println!("{} states pending after {} sec. Continuing.",
                         self.pending_count(),
                         method_start.elapsed().as_secs());
            }

            // Shrink or grow block if necessary.
            if block_elapsed < 2 { block_size = 3 * block_size / 2; }
            else if block_elapsed > 10 { block_size = max(1, block_size / 2); }
        }
    }

    /// A helper that verifies examples exist for all `sometimes` properties and no counterexamples
    /// exist for any `always`/`eventually` properties.
    fn assert_properties(&self) -> &Self
    where M::Action: Debug,
          M::State: Debug,
    {
        for p in self.model().properties() {
            match p.expectation {
                Expectation::Always => self.assert_no_counterexample(p.name),
                Expectation::Eventually => self.assert_no_counterexample(p.name),
                Expectation::Sometimes => { self.assert_example(p.name); },
            }
        }
        self
    }

    /// Panics if an example is not found. Otherwise returns a path to the example.
    fn assert_example(&self, name: &'static str) -> Path<M::State, M::Action> {
        if let Some(path) = self.example(name) {
            return path;
        }
        assert!(self.is_done(),
                "Example for '{}' not found, but model checking is incomplete.", name);
        panic!("Example for '{}' not found. `stateright::explorer` may be useful for debugging.", name);
    }

    /// Panics if a counterexample is not found. Otherwise returns a path to the counterexample.
    fn assert_counterexample(&self, name: &'static str) -> Path<M::State, M::Action> {
        if let Some(path) = self.counterexample(name) {
            return path;
        }
        assert!(self.is_done(),
                "Counterexample for '{}' not found, but model checking is incomplete.", name);
        panic!("Counterexample for '{}' not found. `stateright::explorer` may be useful for debugging.", name);
    }

    /// Panics if an example is found.
    fn assert_no_example(&self, name: &'static str)
    where M::Action: Debug,
          M::State: Debug,
    {
        if let Some(example) = self.example(name) {
            let last_state = format!("{:#?}", example.last_state());
            let actions = example.into_actions()
                .iter()
                .map(|a| format!("{:?}", a))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("Example for '{}' found.\n\n== ACTIONS ==\n{}\n\n== LAST STATE ==\n{}",
                   name, actions, last_state);
        }
        assert!(self.is_done(),
                "Example for '{}' not found, but model checking is incomplete.",
                name);
    }

    /// Panics if a counterexample is found.
    fn assert_no_counterexample(&self, name: &'static str)
    where M::Action: Debug,
          M::State: Debug,
    {
        if let Some(counterexample) = self.counterexample(name) {
            let last_state = format!("{:#?}", counterexample.last_state());
            let actions = counterexample.into_actions()
                .iter()
                .map(|a| format!("{:?}", a))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("Counterexample for '{}' found.\n\n== ACTIONS ==\n{}\n\n== LAST STATE ==\n{}",
                   name, actions, last_state);
        }
        assert!(self.is_done(),
                "Counterexample for '{}' not found, but model checking is incomplete.",
                name);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_util::linear_equation_solver::LinearEquation;

    #[test]
    fn can_build_path_from_fingerprints() {
        let fp = |a: u8, b: u8| fingerprint(&(a, b));
        let model = LinearEquation { a: 2, b: 10, c: 14 };
        let fingerprints = VecDeque::from(vec![
            fp(0, 0),
            fp(0, 1),
            fp(1, 1),
            fp(2, 1), // final state
        ]);
        let path = Path::from_model_and_fingerprints(&model, fingerprints.clone());
        assert_eq!(
            path.last_state(),
            &(2,1));
        assert_eq!(
            path.last_state(),
            &Path::final_state(&model, fingerprints).unwrap());
    }
}
