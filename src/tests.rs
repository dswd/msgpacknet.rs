use super::*;

use test::Bencher;
use std::time::Duration;


struct DummyServer {
    id: u64
}

impl Callback<u64, u64, u64> for DummyServer {
    fn node_id(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.id
    }

    fn create_init_msg(&self, _node: &Node<u64, u64, u64>) -> u64 {
        self.id
    }

    fn node_id_from_init_msg(&self, _node: &Node<u64, u64, u64>, id: &u64) -> u64 {
        *id
    }

    fn connection_timeout(&self, _node: &Node<u64, u64, u64>) -> Duration {
        Duration::from_secs(1)
    }

    fn on_connected(&self, _node: &Node<u64, u64, u64>, _id: &u64) {
    }

    fn on_disconnected(&self, _node: &Node<u64, u64, u64>, _id: &u64) {
    }

    fn handle_message(&self, node: &Node<u64, u64, u64>, src: &u64, msg: u64) {
        node.send(*src, &msg).expect("Failed to send");
    }
}



/*#[bench]
fn bench_full_request(b: &mut Bencher) {
    let (server, server_join) = TcpServer::open("localhost:0").expect("Failed to open");
    let target = EntityAddress::new(server.address(), Obj::Null);
    let request = Obj::Null;
    b.iter(|| {
        let (client, client_join) = TcpServer::open("localhost:0").expect("Failed to open");
        let pending = client.request(target.clone(), request.clone()).expect("Failed to send request");
        pending.wait();
        client.close().expect("Close failed");
        let _ = client_join.join().expect("Join failed");
    });
    server.close().expect("Close failed");
    let _ = server_join.join().expect("Join failed");
}*/
