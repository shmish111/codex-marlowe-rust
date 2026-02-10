# Marlowe language V2

| Version |  |
| :---- | :---- |
| 1.0 | 5 January 2026\. Initial document |

### Rationale and process

As the Marlowe ecosystem reaches maturity, with the deployment of the TS-SDK and other components, it is the right time to review the constructs and behaviour of the core language and its supporting platform, and to scope out and design its successor, Marlowe v2. We envisage improving Marlowe in a number of ways:

*Simplification* of aspects of the original language. This might include simplifying the “`When`” construct so that the contract will by default close on timeout. Generalising some constructs, such as a “`When`” that awaits a set of actions, could substantially simplify the expression of a common programming pattern.

Making the language *more expressive*. This could include adding new control constructs, such as bounded loops, and primitives, e.g. for crypto operations. It would also include conceptual additions, such as sub-languages for time arithmetic or token minting.

Improving the *scalability* of the language: an example of this would be to change the behaviour of a contract on close to optimise performance with larger numbers of contract participants.

Addressing the *security* of the language, which could be improved by checking types of values, so that e.g. addition only takes place between the same “kind” of number.

The discussions of the working group will take place in a series of Google meet  meetings. Discussions will be open, and so recorded and minuted, and supplemented by asynchronous online discussions in the [Marlowe SIG channel in Discord](https://discord.com/channels/826816523368005654/1239607206446497843). 

| 5 January | Finalise group membership, circulate an agenda for the first meeting, together with this draft discussion document. |
| :---- | :---- |
| 20 January | First meeting of the group for 1 hour. Review process and timetable; discuss the document and issues raised in the channel.  After the meeting, revise the discussion document, circulate and gather feedback using google drive. |
| 4 February | Second meeting of the group for 1 hour. After the meeting, produce a final draft document, ideally with a rationale of each one and an explanation of dependencies between them. |
| 17 February | Final meeting of the group for 1 hour. Agree priorities for the changes suggested in the document. Deliver final document. |

# Proposed changes

These changes include changes to particular constructs of the language, and more fundamental changes to the model embodied by language itself.

## Language constructs

1. Modify When so that the continuation on timeout is Close. This simplifies the language and means that programs are more linear, and semantics simpler. It also seems to fit with current usage, though it does require the addition of the Sleep construct below.  
   1. We can use Notify (TimeIntervalStart \> timeout) to approximate the semantics of the current timeout branch, as indicated in [this MIP draft](https://github.com/input-output-hk/MIPs/pull/17).   
2. Generalising the When construct to trigger on all of a set of actions.   
   1. A question here is how to disentangle things when only a subset of actions has happened at timeout. Under the existing model, moving to the timeout continuation might not make sense in this intermediate state, but if Close is the timeout option, this would seem to be sensible in most cases.  
   2. This has the advantage of semantic atomicity of multiple inputs, such as  choice and deposit.  
   3. It also supports a validator optimization, with multiple input processing in a single transaction, leading to significant compression of the contract.  
3. Removal of shadowing logic in the When semantics and usage of direct indexing in as part of the Input.  
   1. Currently when an Input is provided an elaborate matching strategy is executed to pick the first Case which matches the input. The semantics of that matching is surprisingly complex and costs a lot of on-chain resources.  
   2. We propose an addition of an index to the Input which will indicate directly which Case the user intends to pick. We argue that the semantics of `When` will be more direct \- it contains a set of actions from which any one can be picked without influencing each other. Do we lose any significant capability by doing this?  
   3. If we introduce indices then we can actually extend the Case so it could contain a set of inputs which should be executed atomically. This could significantly reduce sizes of many contracts and idioms (for example dynamic amount deposit is modeled as Choice \+ Deposit).  
      In other words we propose to add to Case an extra list Case Action \[Action\] Contract (this could require an atomic set of actions from a different parties).  
   4. If we use explicit indices then the list of `Case`’s can be in the future subject to some form of compression as well where only a specific entry has to be delivered uncompressed to the chain.  
4. Add a construct so that execution of the contract will Sleep for a given period (or up to a specified time). This was previously given by a When with an empty list of clauses, which could only time out.  
   1. In the MIP there is a discussion around \`Assert\` (even when we don't introduce it directly it can be modeled by direct division by zero ;-)) and \`Notify\` as an option.  
5. Exceptions, for example in the case of division by zero.  
   1. This could be “guarded” with a value to use in case the denominator is zero, but arguably this leads to cluttered, defensive programming. Currently Marlowe does this in the case of lookup for an undefined identifier, and can lead to outcomes that are unexpected and hard to reason about.   
   2. It would be possible to make this (or all) exception(s) Close the contract. This “graceful shutdown” would be in line with the proposal to Close on timeout.  
   3. A third option is for exceptions to be a “no op” in the context of an enclosing When construct: this is discussed in more detail in the next top-level item.  
6. Allow the When construct to proceed on *any* clause that succeeds, rather than just the first. This would make the When **non-deterministic,** but is consistent with analysis of the contracts, for example, where all possible paths are explored.  
   1. This high-level semantics is also consistent with a lower-level view in which computation can exceed constraints, and so not allowing the computation to proceed along that branch; in such a case it is possible for computation to proceed along other branches.  
   2. This approach is consistent with interpreting exceptions as “no op”s: if an exception occurs in a branch, other branches can be explored.  
7. EntryDeposit input \- this type of input could replace the cumbersome and hard to use Open roles. This input would effectively be an address variable binding (a variable of type “address”), which could later be used as reference to a particular Party. More details [here](https://github.com/input-output-hk/MIPs/discussions/9).   
8. An enhanced Time data type, including  
   1. Time arithmetic: operators for combination of time values  
   2. Time relative to events, including the start of contract or the minTime.  
9. Extended parameterisation.  
   1. Parameters in Bound (i.e., not just constants), so that bounds can range between variables instead of numbers.  
   2. Roles as parameters, so that the role in a choice etc. can be a variable.  
   3. Deposit a non-predefined amount  
   4. This and the previous point are related to prior discussion of Extended Marlowe and parametrisation [here](https://github.com/marlowe-lang/MIPs/discussions/6).  
10. Support for token manipulation  
    1. Mint and distribute new role tokens during the course of a contract, instead of at the beginning. See [here](https://github.com/marlowe-lang/MIPs/discussions/12) for examples of use cases.  
    2. Mint utility tokens during the course of a contract.  
    3. Manage tokens together as Value, not separately.  
11. Additional core primitive functions  
    1. Bit arithmetic.  
    2. Function for computing a hash of an integer.  
    3. Other cryptographic primitives  
12. Add constructs for repetition, including, for example  
    1. Fold-based recursion.  
    2. Iteration and loops.  
    3. Note that this is most easily supported by the change to SISO control flow suggested below.   
13. Add support for naming  
    1. Naming (sub-)contracts adds explicit sharing.  
    2. This could be generalised to adding function definitions to the language, with a range of parameter types (see discussion of parametrisation above).  
    3. Also see discussion of sharing in the next section.  
14. Add support for further data types  
    1. In the context of blockchain and its limited resources \`Set\` and \`Map\` could be really useful because they "compress" well with Merkleized Patricia Tries (logarithmic membership proofs and basic operations).  
    2. Lists/Sorted List/Heaps  (with Merkleized flavors) probably could be useful on the chain as well but they require assessment of their specific operation proof sizes.

## Language model

15. Control flow: is it possible to transform Marlowe into a single-in, single-out model with timeouts? This makes Marlowe more like a traditional block-structured language.   
                     ![][image1]  
    1. Pay and let do this already. It is a matter of transforming everything else.  
    2. This makes supporting (bounded) recursion much more straightforward.  
    3. This has an impact on sharing (see below): in the continuation model it is often the case that the same continuation is used multiple times in a contract; in this approach it is simply the same (program) continuation.  
16. Sharing: one way of avoiding duplication in Marlowe contracts is to allow some sort of let for contracts themselves, so that duplication is explicit in duplicated use of a label. This is a potential advantage for readability as well as potentially supporting efficiency in an implementation through sharing.  
    1. Note: the readability is given by embedding Marlowe in a language where (sub-)contracts can be named, but this is lost when the contract is transformed into Marlowe.  
    2. Aside: this is a flaw in Faustus. The source language has sharing, but this is lost in translation to Marlowe.  
17. Type checker: beside the static analysis approach to the contract validation we could also introduce a regular type checker \- even in the current version it could be helpful. More details [here](https://github.com/input-output-hk/MIPs/discussions/11).   
18. Marlowe contracts currently include *metadata* e.g. to improve the user experience within the Playground and other tools. A discussion of how this could be systematised and extended is [here](https://github.com/marlowe-lang/MIPs/discussions/7).  
    1. This metadata-enabled Marlowe code could be the output from a compilation process that takes an extended “surface” language and translates it into Marlowe for on-chain execution.  
19. Make Marlowe chain-agnostic. Details presented [here](https://github.com/marlowe-lang/MIPs/discussions/3).  
20. Marlowe State Channel (Layer 2 solution).  
    1. When designing the language we should probably focus also on a somewhat non obvious execution model which could give this technology an edge in the context of the blockchain.  
    2. We could look at the L1 as a layer which should be rather sparsely used to settle the necessary steps or cashflows or to resolve conflicts.  
21. Adopt an explicit state machine model. Actions trigger transitions between states, with accompanying outputs (payments etc).  
    1. This would still be based on the local accounts model.  
    2. Naively, this loses the finite lifetime property, but it would be possible to reintroduce this through bounding the time or number of steps. Alternatively the crucial thing would be to ensure that the contract never “gets stuck”.  
    3. Could support this proposal directly on chain with multiple validators, one per state, and off-chain in the Marlowe Runner.

# Implementation changes

22. Validator changes to reduce on-chain costs for specific contracts and Marlowe idioms.  
    1. This is covered elsewhere, e.g. by the proposal to treat multiple inputs in a single step.  
    2. Autowithdrawls  
23. Configurable fees for contract runners.  
24. Set payment PKH for a role, so that sending to the payout validator can be bypassed.  
25. Payment to scripts, which needs the capability to attach datum or datum hash to output.  
26. State compression. The memory limitations of the Marlowe Validator on-chain present considerable challenges due to memory constraints. While we must also take into account the computational load, theoretically, state compression through providing a subset of the state could reduce the datum decoding volume. More details [here](https://github.com/marlowe-lang/MIPs/discussions/13).  
27. There may be substantial utility in expanding the Validator datum type, enabling it to convey additional data that could be passed as datum to the payout UTxOs and payout script. The Marlowe validator should solely ensure the preservation of this value across its execution thread on the chain. More details [here](https://github.com/marlowe-lang/MIPs/discussions/10).  
28. Optimistic input submission. One of the limitations of the current execution flow of Marlowe contracts on chain is its inability to easily handle different orderings of Inputs which permutations (like deposits) preserve the same semantics. Details [here](https://github.com/marlowe-lang/MIPs/discussions/4).  
    1. Potentially having independently verifiable choices (signed choices)  
29. Allowing splitting Close into several transactions or into several outputs  
    

[image1]: <data:image/png;base64,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>