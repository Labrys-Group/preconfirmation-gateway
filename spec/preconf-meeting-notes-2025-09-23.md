Sep 23, 2025

## Preconf Q & A

Invited [Justin Taylor](mailto:justin@labrys.io) [jasonvranek@gmail.com](mailto:jasonvranek@gmail.com) [Aidan Kinzett](mailto:aidan@labrys.io)

Attachments [Preconf Q & A](https://www.google.com/calendar/event?eid=MnMxYnQ0NGt1ZGZzb2M0Nmg1Nzd0MHBwcW8ganVzdGluQGxhYnJ5cy5pbw) 

Meeting records [Transcript](?tab=t.9trfz8nj7p6m) [Recording](https://drive.google.com/file/d/1oHDS3QJ27anafHMTIjN8HLyGRaG_LPAg/view?usp=drive_web) 

### Summary

Jason Vranek discussed the gateway's need to specify a whitelist of BLS public keys to prevent proposers from self-building and cutting out the gateway and explained the block submission timing to the builder or relay, noting that builders benefit from more time to extract value from a block. Jason Vranek provided a detailed walkthrough of the gateway's operations and the commitments API, which is user-facing, and explained the slashing process, emphasizing that the gateway should post constraints once per slot within an allotted time. Justin Taylor confirmed they have a JSON RPC framework set up with skeleton methods for external calls and a PostgreSQL database for scalability, and that Aidan Kinzett has access to the Telegram group and Notion documents, including pre-conf inclusion specifications, GitHub links, and the diagram Jason Vranek used during the explanation.

### Details

* **Gateway-Builder Interaction and Security** Jason Vranek discussed the motivation behind the gateway's need to specify a whitelist of BLS public keys to prevent proposers from self-building and cutting out the gateway. He explained that builders typically have associated public keys, which should be accessible to the gateway, initially suggesting a config file for demo purposes. Jason Vranek also noted that Michael is more familiar with the builder and relay side of things, suggesting that they could provide further insights on the matter.

* **Block Submission Timing and Epoch Prediction** Justin Taylor inquired about the timing of submissions to the block builder or relay, noting that it generally occurs 8 seconds into every slot. Jason Vranek clarified that builders benefit from more time to extract value from a block, and proposers begin calling the \`get header\` endpoint from the relay around the 8-second mark to allow enough time for signing and broadcasting ([00:01:23](?tab=t.9trfz8nj7p6m#heading=h.z8rksjph8t4l)). Justin Taylor also asked about seeing the entire next epoch through the look-ahead window, to which Jason Vranek explained that while the current epoch is known and a solid prediction for the next is possible, changes can occur due to slashing or balance changes, which EIP 7917 aims to make fully deterministic ([00:02:55](?tab=t.9trfz8nj7p6m#heading=h.lsl3xvicg1na)).

* **Gateway Operations and Commitment Workflow** Jason Vranek provided a detailed walkthrough of the gateway's operations, starting with proposers registering on-chain and sidecars checking the beacon state to sign delegations to configured gateways ([00:05:51](?tab=t.9trfz8nj7p6m#heading=h.97turhb11mcm)). The relay then broadcasts these delegations, which the gateway periodically tracks to identify relevant slots ([00:07:06](?tab=t.9trfz8nj7p6m#heading=h.4qspiaodz91g)). Jason Vranek further elaborated on the commitments API, which is user-facing, explaining that wallets abstract away the complexity of contacting gateways and posting commitment requests ([00:08:36](?tab=t.9trfz8nj7p6m#heading=h.415k7zmnzrod)). Upon receiving a request, the gateway verifies its type and payload format, and if capable, creates and signs a matching constraint, with timing enforcement by the relay for accountability and easier slashing ([00:09:55](?tab=t.9trfz8nj7p6m#heading=h.xenwfimzzyb0)).

* **Slashing and Transaction Validation** Jason Vranek explained the slashing process, emphasizing that the gateway should post constraints once per slot within an allotted time, with the relay enforcing this timing to simplify slashing ([00:09:55](?tab=t.9trfz8nj7p6m#heading=h.xenwfimzzyb0)). He detailed how the relay makes constraints available to whitelisted builders, who then build blocks by appending included transactions and provide Merkel inclusion proofs, which the relay verifies ([00:11:18](?tab=t.9trfz8nj7p6m#heading=h.8nnx5cgcll9n)). Jason Vranek also addressed how the gateway determines its ability to handle a commitment, primarily by checking the gas limit and the number of transactions it can accept. He added that while some invalid transactions could be caught at the slasher level, verifying transaction validity earlier might be considered for future iterations ([00:12:39](?tab=t.9trfz8nj7p6m#heading=h.bqj1skvwimf9)).

* **Gateway Development Progress and Resources** Justin Taylor confirmed that they have a JSON RPC framework set up with skeleton methods for external calls like \`get slot commitment request\` and a PostgreSQL database for scalability ([00:14:18](?tab=t.9trfz8nj7p6m#heading=h.o5yz049cs356)). Jason Vranek expressed hope for type reuse across different actors on the relay side. Justin Taylor confirmed that Aidan Kinzett has access to the Telegram group and Notion documents, including pre-conf inclusion specifications, GitHub links, and the diagram Jason Vranek used during the explanation ([00:15:58](?tab=t.9trfz8nj7p6m#heading=h.kza3y62plgca)). Jason Vranek highlighted two open PRs, one on the URC side and another in the pre-MP specs repo, which explains changes to the gateway for validating BLS signatures ([00:17:30](?tab=t.9trfz8nj7p6m#heading=h.oihluhhfy1qj)).

### Suggested next steps

*No suggested next steps were found for this meeting.*

*You should review Gemini's notes to make sure they're accurate. [Get tips and learn how Gemini takes notes](https://support.google.com/meet/answer/14754931)*

*Please provide feedback about using Gemini to take notes in a [short survey.](https://google.qualtrics.com/jfe/form/SV_9vK3UZEaIQKKE7A?confid=odmr2FwNGLxEElKHaeyqDxIROAIIigIgABgECA&detailid=unspecified)*